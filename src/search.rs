use std::time::{Duration, Instant};

use crate::board::{Board, PieceKind, Player};
use crate::move_gen::{Move, generate_moves};
use crate::nnue::{Accumulator, NnueWeights};
use crate::zobrist::{TTEntry, TranspositionTable, ZobristTable};

// 探索の制限時間や深さの条件
pub struct SearchLimits {
    pub max_time: Duration,
    pub max_depth: u8,
}

struct Search<'zt, 'tt, 'nw> {
    z_table: &'zt ZobristTable,
    tt: &'tt mut TranspositionTable,
    nnue_weights: &'nw NnueWeights,
    start_time: Instant,
    max_time: Duration,
    nodes: usize,
    history: Vec<u64>, // 千日手判定のためのハッシュ履歴
}

// --- メイン探索関数 ---
// 外部（UIや対局プロトコル）から呼ばれるエントリーポイントです。
pub fn search_best_move(
    board: &Board,
    z_table: &ZobristTable,
    tt: &mut TranspositionTable,
    nnue_weights: &NnueWeights,
    limits: &SearchLimits,
    game_history: &[u64], // 実際の対局でのこれまでの履歴
) -> Move {
    let start_time = Instant::now();
    let mut best_move = Move(0); // 最終的に返す最善手
    let mut best_score = 0;

    // 1. ルートノードでの初期計算
    let current_hash = board.compute_initial_hash(z_table);
    let active_features = board.extract_all_features();
    let initial_acc = Accumulator::refresh(nnue_weights, &active_features);

    // 2. 反復深化 (Iterative Deepening) のループ
    for depth in 1..=limits.max_depth {
        let mut search = Search {
            z_table,
            tt,
            nnue_weights,
            max_time: limits.max_time,
            nodes: 0,
            start_time,
            history: game_history.to_vec(), // 実際の履歴をコピーして探索用の履歴とする
        };

        // PVS/アルファベータ探索を呼び出す
        let score = search.search_pvs(
            board,
            depth,
            -30000, // 初期アルファ値 (-∞)
            30000,  // 初期ベータ値 (+∞)
            &initial_acc,
            current_hash,
        );

        let nodes_searched = search.nodes;

        // ★ タイムマネジメント ★
        if start_time.elapsed() >= limits.max_time {
            println!("時間切れのため、深さ {} の探索を中断しました。", depth);
            break;
        }

        if let Some(entry) = tt.probe(current_hash) {
            best_move = entry.best_move;
            best_score = score;
        }

        println!(
            "info depth {} score {} nodes {} time {}ms pv ...",
            depth,
            score,
            nodes_searched,
            start_time.elapsed().as_millis()
        );

        if best_score.abs() >= 20000 {
            println!("詰みを発見したので探索を終了します");
            break;
        }
    }

    best_move
}

impl Search<'_, '_, '_> {
    // --- 千日手判定メソッド ---
    fn is_repetition(&self, current_hash: u64) -> bool {
        let len = self.history.len();
        if len >= 2 {
            // 手番が同じ局面だけを比較するため、2手ずつ遡る
            let mut i = len.saturating_sub(2);
            loop {
                if self.history[i] == current_hash {
                    return true;
                }
                if i < 2 {
                    break;
                }
                i -= 2;
            }
        }
        false
    }
    
    // --- 静止探索 (Quiescence Search) ---
    // 激しい手（取る、成る、トライ）だけを底まで読み切る専用の探索関数
    fn search_q(
        &mut self,
        board: &Board,
        mut alpha: i32,
        beta: i32,
        current_acc: &Accumulator,
        current_hash: u64,
        q_depth: i8, // 探索が深くなりすぎるのを防ぐためのカウンター
    ) -> i32 {
        self.nodes += 1;

        // 終局判定
        if let Some(winner) = board.winner() {
            if winner == board.side_to_move {
                return 20000;
            } else {
                return -20000;
            }
        }

        // --- ★ 追加: qsearch内の千日手判定 ---
        if self.is_repetition(current_hash) {
            return 0;
        }

        if self.nodes & 2047 == 0 && self.start_time.elapsed() >= self.max_time {
            return 0; // タイムアウトフラグ
        }

        // 1. Stand-pat (現状維持) のスコア評価
        // これ以上何もしなくても得られる評価値。これがbetaを超えていたら即座にカット(フェイルソフト)
        let stand_pat = current_acc.evaluate(self.nnue_weights);

        if stand_pat >= beta {
            return stand_pat;
        }
        if alpha < stand_pat {
            alpha = stand_pat;
        }

        // 無限ループ対策: 一定以上深く潜ったら打ち切る
        if q_depth < -10 {
            return stand_pat;
        }

        let mut moves = Vec::new();
        generate_moves(board, &mut moves);

        let opponent = board.side_to_move.opponent();
        let opponent_occupied = board.occupied_by(opponent);

        // 2. Move Filtering (激しい手だけを抽出)
        let noisy_moves: Vec<Move> = moves
            .into_iter()
            .filter(|&m| {
                if m.is_drop() {
                    return false;
                } // 持ち駒を打つ手は一旦「静か」とみなす

                let to_bit = 1 << m.sq_to();
                let is_capture = (opponent_occupied & to_bit) != 0;
                let is_promote = m.is_promote();

                // どうぶつ将棋特有: ライオンの敵陣への移動（トライ）も激しい手とみなす
                let is_lion_entering_enemy_zone = m.piece_kind() == PieceKind::Lion
                    && match board.side_to_move {
                        Player::Sente => m.sq_to() < 3,
                        Player::Gote => m.sq_to() > 8,
                    };

                is_capture || is_promote || is_lion_entering_enemy_zone
            })
            .collect();

        // ★追加: 探索を下る前に現在のハッシュを履歴に積む
        self.history.push(current_hash);

        // 3. 激しい手だけをアルファベータ探索で読み切る
        for m in noisy_moves {
            let mut next_board = board.clone();
            let (feature_update, next_hash) = next_board.make_move(m, self.z_table, current_hash);

            let mut next_acc = current_acc.clone();
            next_acc.update(
                self.nnue_weights,
                &feature_update.added[..feature_update.added_count],
                &feature_update.removed[..feature_update.removed_count],
            );

            let score = -self.search_q(
                &next_board,
                -beta,
                -alpha,
                &next_acc,
                next_hash,
                q_depth - 1,
            );

            if score >= beta {
                self.history.pop(); // ★ ベータカットで抜ける時も忘れずに履歴を戻す
                return score;
            }
            if score > alpha {
                alpha = score;
            }
        }

        self.history.pop(); // ★ 全ての探索が終わったら履歴を戻す

        alpha
    }

    // --- PVS (Principal Variation Search) のコア関数 ---
    fn search_pvs(
        &mut self,
        board: &Board,
        depth: u8,
        mut alpha: i32,
        beta: i32,
        current_acc: &Accumulator,
        current_hash: u64,
    ) -> i32 {
        self.nodes += 1;

        if let Some(winner) = board.winner() {
            if winner == board.side_to_move {
                return 20000 + depth as i32;
            } else {
                return -20000 - depth as i32;
            }
        }

        // --- 千日手なら即座に引き分けスコア(0)を返す ---
        if self.is_repetition(current_hash) {
            return 0;
        }

        // 1. 終了条件の確認
        if depth == 0 {
            // ★ 末端ノードに達したら、NNUEを直接呼ばずに静止探索 (qsearch) に移行する
            return self.search_q(board, alpha, beta, current_acc, current_hash, 0);
        }

        // 数千ノードに1回くらいの頻度で時間切れをチェックし、タイムアウトなら即座に抜ける
        if self.nodes & 2047 == 0 && self.start_time.elapsed() >= self.max_time {
            return 0; // タイムアウトフラグとして扱う
        }

        // 2. 置換表 (TT) のルックアップ
        let mut tt_move = None;
        if let Some(entry) = self.tt.probe(current_hash) {
            tt_move = Some(entry.best_move);
            // もし十分な深さまで探索済みで、かつ評価値が境界内ならそのまま返す (TT Cut)
            // ... 省略 ...
        }

        let mut moves = Vec::new();
        generate_moves(board, &mut moves);

        if moves.is_empty() {
            return -20000 - depth as i32;
        }

        if let Some(pv_move) = tt_move
            && let Some(pos) = moves.iter().position(|&m| m == pv_move)
        {
            moves.swap(0, pos);
        }

        let mut best_score = -30000;
        let mut best_move = moves.first().copied().unwrap_or(Move(0));
        let mut is_first_move = true;

        // 探索を下る前に現在のハッシュを履歴に積む
        self.history.push(current_hash);

        for m in moves {
            let mut next_board = board.clone();
            // NNUEのFeatureUpdateとZobristの差分ハッシュを同時に取得！
            let (feature_update, next_hash) = next_board.make_move(m, self.z_table, current_hash);

            let mut next_acc = current_acc.clone();
            next_acc.update(
                self.nnue_weights,
                &feature_update.added[..feature_update.added_count],
                &feature_update.removed[..feature_update.removed_count],
            );

            let score;
            if is_first_move {
                // 最初の手は全幅の窓 (Full Window) で調べる
                score =
                    -self.search_pvs(&next_board, depth - 1, -beta, -alpha, &next_acc, next_hash);
                is_first_move = false;
            } else {
                // ★ Null Window Search (幅1の窓) による高速化 ★
                let null_score = -self.search_pvs(
                    &next_board,
                    depth - 1,
                    -alpha - 1,
                    -alpha,
                    &next_acc,
                    next_hash,
                );

                if null_score > alpha && null_score < beta {
                    score = -self.search_pvs(
                        &next_board,
                        depth - 1,
                        -beta,
                        -null_score,
                        &next_acc,
                        next_hash,
                    );
                } else {
                    score = null_score;
                }
            }

            // αβの更新
            if score > best_score {
                best_score = score;
                best_move = m;
                if score > alpha {
                    alpha = score;
                }
                if alpha >= beta {
                    // ベータカット (枝刈り成功！)
                    break;
                }
            }
        }

        // 探索から戻ってきたら履歴から消す
        self.history.pop();

        // 探索結果を置換表に保存する
        self.tt.store(TTEntry {
            key: current_hash,
            depth,
            score: best_score,
            best_move,
            node_type: 0, // 省略
        });

        best_score
    }
}