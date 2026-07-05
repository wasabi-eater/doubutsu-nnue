#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};
#[cfg(target_arch = "wasm32")]
use web_time::{Duration, Instant};

#[cfg(target_arch = "wasm32")]
use web_sys::console;

use crate::board::{Board, PieceKind, Player, get_piece_index};
use crate::move_gen::{Move, generate_moves};
use crate::nnue::{Accumulator, NnueWeights};
use crate::zobrist::{TTEntry, TranspositionTable, ZobristTable};
use rayon::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub const EXACT: u8 = 0;
pub const LOWER_BOUND: u8 = 1;
pub const UPPER_BOUND: u8 = 2;
const MAX_PLY: usize = 64;

pub struct SearchLimits {
    pub max_time: Duration,
    pub max_depth: u8,
}

struct Search<'a, 'zt, 'tt, 'nw> {
    z_table: &'zt ZobristTable,
    tt: &'tt TranspositionTable,
    nnue_weights: &'nw NnueWeights,
    start_time: Instant,
    max_time: Duration,
    nodes: &'a mut usize,
    history: &'a mut Vec<(u64, Board)>,
    killer_moves: &'tt mut [[Option<Move>; 2]; MAX_PLY],
    aborted: bool,
    shared_abort: Arc<AtomicBool>,
    thread_id: usize,
}

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
    tt: &TranspositionTable,
    nnue_weights: &NnueWeights,
    limits: &SearchLimits,
    game_history: &[(u64, Board)],
) -> (Move, u8) {
    let start_time = Instant::now();
    let shared_abort = Arc::new(AtomicBool::new(false));

    let mut moves = Vec::new();
    generate_moves(board, &mut moves);
    let default_move = moves.first().copied().unwrap_or(Move(0));

    let current_hash = board.compute_initial_hash(z_table);
    let active_features = board.extract_all_features();
    let initial_acc = Accumulator::refresh(nnue_weights, &active_features);

    let num_threads = rayon::current_num_threads().max(1);

    let thread_results: Vec<_> = (0..num_threads)
        .into_par_iter()
        .map(|thread_id| {
            let mut best_move = default_move;
            let mut best_score = 0;
            let mut reached_depth = 0;
            let mut killer_moves = [[None; 2]; MAX_PLY];

            let mut thread_history = game_history.to_vec();
            let mut thread_nodes = 0; // スレッドごとの累計ノード数

            // 制限時間が短い場合には効率が悪いため削除し、全スレッドで全力探索させる
            for depth in 1..=limits.max_depth {
                let mut search = Search {
                    z_table,
                    tt,
                    nnue_weights,
                    max_time: limits.max_time,
                    nodes: &mut thread_nodes, // 参照を渡す
                    start_time,
                    history: &mut thread_history, // 参照を渡す
                    killer_moves: &mut killer_moves,
                    aborted: false,
                    shared_abort: shared_abort.clone(),
                    thread_id,
                };

                let (score, current_best_move) =
                    search.search_pvs(board, depth, 0, -30000, 30000, &initial_acc, current_hash);

                if search.aborted {
                    // 時間切れの場合、その深さの探索は完了していないので、到達深さは1つ前とする
                    if thread_id == 0 && depth > 1 {
                        reached_depth = depth - 1;
                    }
                    break;
                }

                if thread_id == 0 {
                    best_move = current_best_move;
                    best_score = score;
                    reached_depth = depth;


                    let elapsed_ms = start_time.elapsed().as_millis() as u64;
                    let nps = (*search.nodes as u64 * 1000).checked_div(elapsed_ms).unwrap_or(0);

                    #[cfg(target_arch = "wasm32")]
                    console::log_1(
                        &format!(
                            "Depth: {} | Score: {} | Nodes: {} | Time: {}ms | NPS: {} / core",
                            depth, score, *search.nodes, elapsed_ms, nps
                        )
                        .into(),
                    );

                    #[cfg(not(target_arch = "wasm32"))]
                    println!(
                        "Depth: {} | Score: {} | Nodes: {} | Time: {}ms | NPS: {} / core",
                        depth, score, *search.nodes, elapsed_ms, nps
                    );
                }

                // 詰みを発見したら全スレッドを即座に止める
                if score.abs() >= 19000 {
                    shared_abort.store(true, Ordering::Relaxed);
                    break;
                }
            }

            (best_move, reached_depth, best_score)
        })
        .collect();

    let main_result = thread_results[0];
    (main_result.0, main_result.1)
}

impl Search<'_, '_, '_, '_> {
    fn check_time(&mut self) {
        if *self.nodes & 2047 == 0 {
            if self.shared_abort.load(Ordering::Relaxed) {
                self.aborted = true;
            } else if self.thread_id == 0 && self.start_time.elapsed() >= self.max_time {
                self.shared_abort.store(true, Ordering::Relaxed);
                self.aborted = true;
            }
        }
    }

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

    fn can_capture_lion(&self, board: &Board, moves: &[Move]) -> bool {
        let opponent = board.side_to_move.opponent();
        let opponent_lion_idx = get_piece_index(opponent, PieceKind::Lion);
        let opponent_lion_bb = board.piece_bbs[opponent_lion_idx];

        for m in moves {
            if !m.is_drop() && (opponent_lion_bb & (1 << m.sq_to())) != 0 {
                return true;
            }
        }
        false
    }

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
        if self.aborted {
            return 0;
        }
        *self.nodes += 1;

        if let Some(winner) = board.winner() {
            return if winner == board.side_to_move {
                20000 - ply as i32
            } else {
                -20000 + ply as i32
            };
        }

        let mut moves = Vec::new();
        generate_moves(board, &mut moves);

        if self.can_capture_lion(board, &moves) {
            return 20000 - ply as i32;
        }

        if ply > 0 && self.is_repetition(current_hash, board) {
            return 0;
        }

        self.check_time();
        if self.aborted {
            return 0;
        }

        let eval = current_acc.evaluate(self.nnue_weights);
        let stand_pat = if board.side_to_move == Player::Sente {
            eval
        } else {
            -eval
        };

        if stand_pat >= beta {
            return stand_pat;
        }
        if alpha < stand_pat {
            alpha = stand_pat;
        }
        if q_depth < -10 {
            return stand_pat;
        }

        let opponent = board.side_to_move.opponent();
        let opponent_occupied = board.occupied_by(opponent);

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

        noisy_moves.sort_by_cached_key(|&m| {
            let mut move_score = 0;
            let to_bit = 1 << m.sq_to();
            if (opponent_occupied & to_bit) != 0 {
                move_score += 10000 - piece_value(m.piece_kind());
            }
            if m.is_promote() {
                move_score += 5000;
            }
            -move_score
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

            if self.aborted {
                self.history.pop();
                return 0;
            }
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
    ) -> (i32, Move) {
        if self.aborted {
            return (0, Move(0));
        }
        *self.nodes += 1;
        let alpha_orig = alpha;

        if let Some(winner) = board.winner() {
            let score = if winner == board.side_to_move {
                20000 - ply as i32
            } else {
                -20000 + ply as i32
            };
            return (score, Move(0));
        }

        let mut moves = Vec::new();
        generate_moves(board, &mut moves);
        if moves.is_empty() {
            return (-20000 + ply as i32, Move(0));
        }

        if self.can_capture_lion(board, &moves) {
            let opponent = board.side_to_move.opponent();
            let opponent_lion_idx = get_piece_index(opponent, PieceKind::Lion);
            let opponent_lion_bb = board.piece_bbs[opponent_lion_idx];
            for m in &moves {
                if !m.is_drop() && (opponent_lion_bb & (1 << m.sq_to())) != 0 {
                    return (20000 - ply as i32, *m);
                }
            }
        }

        if ply > 0 && self.is_repetition(current_hash, board) {
            return (0, Move(0));
        }

        if depth == 0 {
            return (
                self.search_q(board, ply, alpha, beta, current_acc, current_hash, 0),
                Move(0),
            );
        }

        self.check_time();
        if self.aborted {
            return (0, Move(0));
        }

        let mut tt_move = None;
        if let Some(entry) = self.tt.probe(current_hash) {
            tt_move = Some(entry.best_move);
            if entry.depth >= depth {
                let mut score = entry.score;
                if score > 19000 {
                    score -= ply as i32;
                } else if score < -19000 {
                    score += ply as i32;
                }

                if score != 0 {
                    if entry.node_type == EXACT {
                        return (score, entry.best_move);
                    }
                    if entry.node_type == LOWER_BOUND && score >= beta {
                        return (score, entry.best_move);
                    }
                    if entry.node_type == UPPER_BOUND && score <= alpha {
                        return (score, entry.best_move);
                    }
                }
            }
        }

        let opponent = board.side_to_move.opponent();
        let opponent_occupied = board.occupied_by(opponent);

        moves.sort_by_cached_key(|&m| {
            if Some(m) == tt_move {
                return -30000;
            }
            if !m.is_drop() {
                let to_bit = 1 << m.sq_to();
                if (opponent_occupied & to_bit) != 0 {
                    // LVA (Least Valuable Attacker): 弱い駒で取る手ほど優先する
                    let move_score = 10000 - piece_value(m.piece_kind());
                    return -move_score;
                }
                if m.is_promote() {
                    return -5000;
                }
            }
            if ply < MAX_PLY {
                if Some(m) == self.killer_moves[ply][0] {
                    return -900;
                } else if Some(m) == self.killer_moves[ply][1] {
                    return -800;
                }
            }
            0
        });

        // Lazy SMPは、ロックフリー置換表(TT)の非同期共有だけで十分に機能します。

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
                let (s, _) = self.search_pvs(
                    &next_board,
                    depth - 1,
                    ply + 1,
                    -beta,
                    -alpha,
                    &next_acc,
                    next_hash,
                );
                score = -s;
                is_first_move = false;
            } else {
                let (ns, _) = self.search_pvs(
                    &next_board,
                    depth - 1,
                    ply + 1,
                    -alpha - 1,
                    -alpha,
                    &next_acc,
                    next_hash,
                );
                let null_score = -ns;
                if self.aborted {
                    self.history.pop();
                    return (0, Move(0));
                }
                if null_score > alpha && null_score < beta {
                    let (s, _) = self.search_pvs(
                        &next_board,
                        depth - 1,
                        ply + 1,
                        -beta,
                        -null_score,
                        &next_acc,
                        next_hash,
                    );
                    score = -s;
                } else {
                    score = null_score;
                }
            }

            if self.aborted {
                self.history.pop();
                return (0, Move(0));
            }

            if score > best_score {
                best_score = score;
                best_move = m;
                if score > alpha {
                    alpha = score;
                }
                if alpha >= beta {
                    let is_capture = !m.is_drop() && (opponent_occupied & (1 << m.sq_to())) != 0;
                    if !is_capture && ply < MAX_PLY && self.killer_moves[ply][0] != Some(m) {
                        self.killer_moves[ply][1] = self.killer_moves[ply][0];
                        self.killer_moves[ply][0] = Some(m);
                    }
                    break;
                }
            }
        }

        self.history.pop();

        if !self.aborted {
            let node_type = if best_score <= alpha_orig {
                UPPER_BOUND
            } else if best_score >= beta {
                LOWER_BOUND
            } else {
                EXACT
            };

            let mut tt_score = best_score;
            if tt_score > 19000 {
                tt_score += ply as i32;
            } else if tt_score < -19000 {
                tt_score -= ply as i32;
            }

            if tt_score != 0 {
                self.tt.store(TTEntry {
                    key: current_hash,
                    depth,
                    score: tt_score,
                    best_move,
                    node_type,
                });
            }
        }

        (best_score, best_move)
    }
}
