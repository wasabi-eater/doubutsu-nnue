use std::time::{Duration, Instant};

use crate::board::{Board, PieceKind, Player};
use crate::move_gen::{Move, generate_moves};
use crate::nnue::{Accumulator, NnueWeights};
use crate::zobrist::{TTEntry, TranspositionTable, ZobristTable};

// --- 置換表のノードタイプ定数 ---
const EXACT: u8 = 0;
const LOWER_BOUND: u8 = 1; // ベータカット (これ以上良い手がある)
const UPPER_BOUND: u8 = 2; // アルファカット (悪い手だった)

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
    history: Vec<(u64, Board)>, // 千日手判定のためのハッシュと盤面の履歴
}

// 駒の価値 (Move Ordering用)
fn piece_value(kind: PieceKind) -> i32 {
    match kind {
        PieceKind::Chick => 100,
        PieceKind::Hen => 300,
        PieceKind::Giraffe => 400,
        PieceKind::Elephant => 400,
        PieceKind::Lion => 10000,
    }
}

pub fn search_best_move(
    board: &Board,
    z_table: &ZobristTable,
    tt: &mut TranspositionTable,
    nnue_weights: &NnueWeights,
    limits: &SearchLimits,
    game_history: &[(u64, Board)], // 実際の対局でのこれまでの履歴
) -> Move {
    let start_time = Instant::now();

    // ★フェイルセーフ: 万が一探索が手を選べなかった場合でも盤面が壊れないよう、
    // 未定義のMove(0)ではなく、合法手のどれかをデフォルトに設定しておく
    let mut moves = Vec::new();
    generate_moves(board, &mut moves);
    let mut best_move = moves.first().copied().unwrap_or(Move(0));

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

        let score = search.search_pvs(board, depth, 0, -30000, 30000, &initial_acc, current_hash);

        let nodes_searched = search.nodes;

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
    fn is_repetition(&self, current_hash: u64, current_board: &Board) -> bool {
        let len = self.history.len();
        if len >= 2 {
            let mut i = len.saturating_sub(2);
            loop {
                if self.history[i].0 == current_hash && self.history[i].1 == *current_board {
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
        ply: usize,
        mut alpha: i32,
        beta: i32,
        current_acc: &Accumulator,
        current_hash: u64,
        q_depth: i8,
    ) -> i32 {
        self.nodes += 1;

        // 終局判定
        if let Some(winner) = board.winner() {
            return if winner == board.side_to_move {
                20000
            } else {
                -20000
            };
        }

        if ply > 0 && self.is_repetition(current_hash, board) {
            return 0;
        }

        if self.nodes & 2047 == 0 && self.start_time.elapsed() >= self.max_time {
            return 0;
        }

        let stand_pat = current_acc.evaluate(self.nnue_weights);
        if stand_pat >= beta {
            return stand_pat;
        }
        if alpha < stand_pat {
            alpha = stand_pat;
        }

        if q_depth < -10 {
            return stand_pat;
        }

        let mut moves = Vec::new();
        generate_moves(board, &mut moves);
        let opponent = board.side_to_move.opponent();
        let opponent_occupied = board.occupied_by(opponent);

        // Move Filtering (激しい手だけを抽出)
        let mut noisy_moves: Vec<Move> = moves
            .into_iter()
            .filter(|&m| {
                if m.is_drop() {
                    return false;
                }
                let to_bit = 1 << m.sq_to();
                let is_capture = (opponent_occupied & to_bit) != 0;
                let is_promote = m.is_promote();
                let is_lion_entering = m.piece_kind() == PieceKind::Lion
                    && match board.side_to_move {
                        Player::Sente => m.sq_to() < 3,
                        Player::Gote => m.sq_to() > 8,
                    };
                is_capture || is_promote || is_lion_entering
            })
            .collect();

        // ★ 静止探索での Move Ordering (取る手を強力に優先)
        noisy_moves.sort_by_cached_key(|&m| {
            let mut move_score = 0;
            let to_bit = 1 << m.sq_to();
            if (opponent_occupied & to_bit) != 0 {
                // MVV-LVA: 取れる駒の価値が高く、取る側の駒の価値が低いほど優先
                move_score += 10000 - piece_value(m.piece_kind());
            }
            if m.is_promote() {
                move_score += 5000;
            }
            -move_score // 降順ソート
        });

        self.history.push((current_hash, board.clone()));

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
                ply + 1,
                -beta,
                -alpha,
                &next_acc,
                next_hash,
                q_depth - 1,
            );

            if score >= beta {
                self.history.pop();
                return score;
            }
            if score > alpha {
                alpha = score;
            }
        }

        self.history.pop();
        alpha
    }

    fn search_pvs(
        &mut self,
        board: &Board,
        depth: u8,
        ply: usize,
        mut alpha: i32,
        beta: i32,
        current_acc: &Accumulator,
        current_hash: u64,
    ) -> i32 {
        self.nodes += 1;
        let alpha_orig = alpha; // ★ 枝刈りタイプ記録用に保存

        if let Some(winner) = board.winner() {
            return if winner == board.side_to_move {
                20000 + depth as i32
            } else {
                -20000 - depth as i32
            };
        }

        if ply > 0 && self.is_repetition(current_hash, board) {
            return 0;
        }

        if depth == 0 {
            return self.search_q(board, ply, alpha, beta, current_acc, current_hash, 0);
        }

        if self.nodes & 2047 == 0 && self.start_time.elapsed() >= self.max_time {
            return 0;
        }

        // ★ 置換表 (TT) による枝刈りの完全実装
        let mut tt_move = None;
        if let Some(entry) = self.tt.probe(current_hash) {
            tt_move = Some(entry.best_move);
            if entry.depth >= depth {
                if entry.node_type == EXACT {
                    return entry.score;
                }
                if entry.node_type == LOWER_BOUND && entry.score >= beta {
                    return entry.score;
                }
                if entry.node_type == UPPER_BOUND && entry.score <= alpha {
                    return entry.score;
                }
            }
        }

        let mut moves = Vec::new();
        generate_moves(board, &mut moves);
        if moves.is_empty() {
            return -20000 - depth as i32;
        }

        // ★ Move Ordering (優先度をつけてソート)
        let opponent = board.side_to_move.opponent();
        let opponent_occupied = board.occupied_by(opponent);
        moves.sort_by_cached_key(|&m| {
            if Some(m) == tt_move {
                return i32::MAX;
            } // TT最善手を最優先

            let mut move_score = 0;
            if !m.is_drop() {
                let to_bit = 1 << m.sq_to();
                if (opponent_occupied & to_bit) != 0 {
                    move_score += 10000 - piece_value(m.piece_kind());
                }
                if m.is_promote() {
                    move_score += 5000;
                }
            }
            -move_score
        });

        let mut best_score = -30000;
        let mut best_move = moves.first().copied().unwrap_or(Move(0));
        let mut is_first_move = true;

        self.history.push((current_hash, board.clone()));

        for m in moves {
            let mut next_board = board.clone();
            let (feature_update, next_hash) = next_board.make_move(m, self.z_table, current_hash);
            let mut next_acc = current_acc.clone();
            next_acc.update(
                self.nnue_weights,
                &feature_update.added[..feature_update.added_count],
                &feature_update.removed[..feature_update.removed_count],
            );

            let score;
            if is_first_move {
                score = -self.search_pvs(
                    &next_board,
                    depth - 1,
                    ply + 1,
                    -beta,
                    -alpha,
                    &next_acc,
                    next_hash,
                );
                is_first_move = false;
            } else {
                let null_score = -self.search_pvs(
                    &next_board,
                    depth - 1,
                    ply + 1,
                    -alpha - 1,
                    -alpha,
                    &next_acc,
                    next_hash,
                );
                if null_score > alpha && null_score < beta {
                    score = -self.search_pvs(
                        &next_board,
                        depth - 1,
                        ply + 1,
                        -beta,
                        -null_score,
                        &next_acc,
                        next_hash,
                    );
                } else {
                    score = null_score;
                }
            }

            if score > best_score {
                best_score = score;
                best_move = m;
                if score > alpha {
                    alpha = score;
                }
                if alpha >= beta {
                    break;
                }
            }
        }

        self.history.pop();

        // ★ 置換表に探索結果を保存
        let node_type = if best_score <= alpha_orig {
            UPPER_BOUND
        } else if best_score >= beta {
            LOWER_BOUND
        } else {
            EXACT
        };

        if best_score.abs() < 19000 {
            // 詰みスコア以外を保存
            self.tt.store(TTEntry {
                key: current_hash,
                depth,
                score: best_score,
                best_move,
                node_type,
            });
        }

        best_score
    }
}
