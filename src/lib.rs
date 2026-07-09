use wasm_bindgen::prelude::*;
use web_time::Duration;

pub mod board;
pub mod game;
pub mod make_move;
pub mod move_gen;
pub mod nnue;
pub mod search;
pub mod zobrist;

use board::Player;
use game::move_to_string;
use move_gen::generate_moves;
use nnue::NnueWeights;
use search::{SearchLimits, search_best_move};
use zobrist::{TranspositionTable, ZobristTable};

use crate::game::{GameManager, board_string};

// --- JS側に公開するゲーム管理クラス ---
#[wasm_bindgen]
pub struct AnimalShogiWasm {
    game_mng: GameManager<'static>,
    tt: TranspositionTable,
    weights: NnueWeights,
}

impl Default for AnimalShogiWasm {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl AnimalShogiWasm {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let z_table = Box::leak(Box::new(ZobristTable::new())) as &_;
        let game_mng = GameManager::new(z_table);
        let tt = TranspositionTable::new(1024 * 512);

        let weight_bytes = include_bytes!("../nnue_weights_public.bin");

        let weights = NnueWeights::load_from_slice(weight_bytes).unwrap_or_else(|e| {
            web_sys::console::error_1(&format!("重みの読み込みエラー: {:?}", e).into());
            NnueWeights::new_dummy()
        });

        Self {
            game_mng,
            tt,
            weights,
        }
    }

    pub fn reset(&mut self) {
        self.game_mng = GameManager::new(self.game_mng.z_table());
        self.tt = TranspositionTable::new(1024 * 512);
    }

    // --- 🤖 AIの行動 ---
    pub fn search_and_apply_move(&mut self, time_limit_ms: u64) -> String {
        if self.game_mng.moves().is_empty() {
            return r#"{"move_text": "投了", "depth": 0}"#.to_string();
        }

        let limits = SearchLimits {
            max_time: Duration::from_millis(time_limit_ms),
            max_depth: 32,
        };

        let (best_move, depth) = search_best_move(
            self.game_mng.board(),
            self.game_mng.z_table(),
            &self.tt,
            &self.weights,
            &limits,
            self.game_mng.history(),
        );

        self.game_mng.make_move(best_move);
        let move_str = move_to_string(best_move);

        format!(
            r#"{{"move_text": "{}", "depth": {}, "from": {}, "to": {}, "is_drop": {}, "kind": {}}}"#,
            move_str,
            depth,
            best_move.sq_from(),
            best_move.sq_to(),
            best_move.is_drop(),
            best_move.piece_kind() as u8
        )
    }

    // --- 👤 人間の行動 (メインスレッドの同期用にも使います) ---
    pub fn apply_human_move(&mut self, from_sq: u8, to_sq: u8) -> bool {
        let mut moves = Vec::new();
        generate_moves(self.game_mng.board(), &mut moves);

        for m in moves {
            if !m.is_drop() && m.sq_from() == from_sq && m.sq_to() == to_sq {
                self.game_mng.make_move(m);
                return true;
            }
        }
        false
    }

    pub fn apply_human_drop(&mut self, kind_val: u8, to_sq: u8) -> bool {
        for m in self.game_mng.moves() {
            if m.is_drop() && m.piece_kind() as u8 == kind_val && m.sq_to() == to_sq {
                self.game_mng.make_move(*m);
                return true;
            }
        }
        false
    }

    // --- 🎮 状態の取得 ---
    pub fn get_winner(&self) -> i32 {
        if let Some(winner) = self.game_mng.winner() {
            if winner == Player::Sente { 1 } else { 2 }
        } else if self.game_mng.is_draw() {
            0 // 引き分け
        } else {
            -1 // 進行中
        }
    }

    pub fn get_turn(&self) -> i32 {
        if self.game_mng.board().side_to_move == Player::Sente {
            1
        } else {
            2
        }
    }

    pub fn get_board_string(&self) -> String {
        board_string(self.game_mng.board())
    }
}
