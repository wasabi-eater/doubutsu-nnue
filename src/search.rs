use std::time::{Duration, Instant};

use crate::board::Board;
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
    history: Vec<u64>, // ★追加: 千日手判定のためのハッシュ履歴
}

// --- メイン探索関数 ---
pub fn search_best_move(
    board: &Board,
    z_table: &ZobristTable,
    tt: &mut TranspositionTable,
    nnue_weights: &NnueWeights,
    limits: &SearchLimits,
    game_history: &[u64], // ★追加: 実際の対局でのこれまでの履歴
) -> Move {
    let start_time = Instant::now();
    let mut best_move = Move(0);
    let mut best_score = 0;

    let current_hash = board.compute_initial_hash(z_table);
    let active_features = board.extract_all_features();
    let initial_acc = Accumulator::refresh(nnue_weights, &active_features);

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

        let score = search.search_pvs(board, depth, -30000, 30000, &initial_acc, current_hash);

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
    }

    best_move
}

impl Search<'_, '_, '_> {
    // --- ★追加: 千日手判定メソッド ---
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

        // --- ★追加: 千日手なら即座に引き分けスコア(0)を返す ---
        if self.is_repetition(current_hash) {
            return 0;
        }

        if depth == 0 {
            return current_acc.evaluate(self.nnue_weights);
        }

        if self.nodes & 2047 == 0 && self.start_time.elapsed() >= self.max_time {
            return 0;
        }

        let mut tt_move = None;
        if let Some(entry) = self.tt.probe(current_hash) {
            tt_move = Some(entry.best_move);
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

        // ★追加: 探索を下る前に現在のハッシュを履歴に積む
        self.history.push(current_hash);

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
                score =
                    -self.search_pvs(&next_board, depth - 1, -beta, -alpha, &next_acc, next_hash);
                is_first_move = false;
            } else {
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

        // ★追加: 探索から戻ってきたら履歴から消す
        self.history.pop();

        self.tt.store(TTEntry {
            key: current_hash,
            depth,
            score: best_score,
            best_move,
            node_type: 0,
        });

        best_score
    }
}
