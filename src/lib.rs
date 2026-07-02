use wasm_bindgen::prelude::*;
use web_time::Duration;

// 既存のモジュールを読み込む
mod board;
mod make_move;
mod move_gen;
mod nnue;
mod search;
mod zobrist;

use board::{Board, PieceKind, Player};
use move_gen::{Move, generate_moves};
use nnue::NnueWeights;
use search::{SearchLimits, search_best_move};
use zobrist::{TranspositionTable, ZobristTable};

// --- ヘルパー関数: 指し手を人間が読める文字列に変換 ---
fn sq_to_string(sq: u8) -> String {
    let col = (b'A' + (sq % 3)) as char;
    let row = (b'1' + (sq / 3)) as char;
    format!("{}{}", col, row)
}

fn move_to_string(m: Move) -> String {
    let from_sq = (m.0 & 0x0F) as u8;
    let to_sq = ((m.0 >> 4) & 0x0F) as u8;
    let kind_val = (m.0 >> 8) & 0x07;
    let is_promote = (m.0 & (1 << 11)) != 0;
    let is_drop = (m.0 & (1 << 12)) != 0;

    let piece_str = match kind_val {
        0 => "ライオン",
        1 => "きりん",
        2 => "ぞう",
        3 => "ひよこ",
        4 => "にわとり",
        _ => "?",
    };

    if is_drop {
        format!("{} に {} を打つ", sq_to_string(to_sq), piece_str)
    } else {
        let prom = if is_promote { "成" } else { "" };
        format!(
            "{} から {} へ移動 ({}{})",
            sq_to_string(from_sq),
            sq_to_string(to_sq),
            piece_str,
            prom
        )
    }
}

// --- JS側に公開するゲーム管理クラス ---
#[wasm_bindgen]
pub struct AnimalShogiWasm {
    board: Board,
    z_table: ZobristTable,
    tt: TranspositionTable,
    weights: NnueWeights,
    history: Vec<(u64, Board)>,
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
        let z_table = ZobristTable::new();
        let tt = TranspositionTable::new(1024 * 512);

        let weight_bytes = include_bytes!("../checkpoints8/nnue_weights_gen200.bin");
        let weights =
            NnueWeights::load_from_slice(weight_bytes).unwrap_or_else(|_| NnueWeights::new_dummy());

        Self {
            board: Board::initial_position(),
            z_table,
            tt,
            weights,
            history: Vec::new(),
        }
    }

    // ゲームを初期状態にリセットする
    pub fn reset(&mut self) {
        self.board = Board::initial_position();
        self.history.clear();
        self.tt = TranspositionTable::new(1024 * 512);
    }

    // --- 🤖 AIの行動 ---
    // AIに思考させて、選んだ手と到達した探索深さをJSON文字列で返す
    pub fn search_and_apply_move(&mut self, time_limit_ms: u64) -> String {
        let mut moves = Vec::new();
        generate_moves(&self.board, &mut moves);
        if moves.is_empty() {
            // 合法手がない場合は投了のJSONを返す
            return r#"{"move_text": "投了", "depth": 0}"#.to_string();
        }

        let limits = SearchLimits {
            max_time: Duration::from_millis(time_limit_ms),
            max_depth: 32,
        };

        let current_hash = self.board.compute_initial_hash(&self.z_table);

        // ★修正: 最善手と到達深さをタプルで受け取る
        let (best_move, depth) = search_best_move(
            &self.board,
            &self.z_table,
            &mut self.tt,
            &self.weights,
            &limits,
            &self.history,
        );

        self.history.push((current_hash, self.board.clone()));
        self.board.make_move(best_move, &self.z_table, current_hash);

        let move_str = move_to_string(best_move);
        format!(r#"{{"move_text": "{}", "depth": {}}}"#, move_str, depth)
    }

    // --- 👤 人間の行動 ---
    pub fn apply_human_move(&mut self, from_sq: u8, to_sq: u8) -> bool {
        let mut moves = Vec::new();
        generate_moves(&self.board, &mut moves);

        for m in moves {
            let m_from = (m.0 & 0x0F) as u8;
            let m_to = ((m.0 >> 4) & 0x0F) as u8;
            let is_drop = (m.0 & (1 << 12)) != 0;

            if !is_drop && m_from == from_sq && m_to == to_sq {
                let current_hash = self.board.compute_initial_hash(&self.z_table);
                self.history.push((current_hash, self.board.clone()));
                self.board.make_move(m, &self.z_table, current_hash);
                return true;
            }
        }
        false
    }

    pub fn apply_human_drop(&mut self, kind_val: u8, to_sq: u8) -> bool {
        let mut moves = Vec::new();
        generate_moves(&self.board, &mut moves);

        for m in moves {
            let m_to = ((m.0 >> 4) & 0x0F) as u8;
            let m_kind = ((m.0 >> 8) & 0x07) as u8;
            let is_drop = (m.0 & (1 << 12)) != 0;

            if is_drop && m_kind == kind_val && m_to == to_sq {
                let current_hash = self.board.compute_initial_hash(&self.z_table);
                self.history.push((current_hash, self.board.clone()));
                self.board.make_move(m, &self.z_table, current_hash);
                return true;
            }
        }
        false
    }

    // --- 🎮 状態の取得 ---
    pub fn get_winner(&self) -> i32 {
        if let Some(winner) = self.board.winner() {
            if winner == Player::Sente { 1 } else { 2 }
        } else {
            let current_hash = self.board.compute_initial_hash(&self.z_table);
            let count = self
                .history
                .iter()
                .filter(|&&(h, ref b)| h == current_hash && *b == self.board)
                .count();
            if count >= 2 {
                return 0; // 引き分け
            }

            let mut moves = Vec::new();
            generate_moves(&self.board, &mut moves);
            if moves.is_empty() {
                if self.board.side_to_move == Player::Sente {
                    return 2;
                } else {
                    return 1;
                }
            }

            -1 // 進行中
        }
    }

    pub fn get_turn(&self) -> i32 {
        if self.board.side_to_move == Player::Sente {
            1
        } else {
            2
        }
    }

    pub fn get_board_string(&self) -> String {
        let mut s = String::new();
        let piece_str = |p: Player, k: PieceKind| -> &'static str {
            match (p, k) {
                (Player::Sente, PieceKind::Lion) => " L ",
                (Player::Gote, PieceKind::Lion) => " l ",
                (Player::Sente, PieceKind::Giraffe) => " G ",
                (Player::Gote, PieceKind::Giraffe) => " g ",
                (Player::Sente, PieceKind::Elephant) => " E ",
                (Player::Gote, PieceKind::Elephant) => " e ",
                (Player::Sente, PieceKind::Chick) => " C ",
                (Player::Gote, PieceKind::Chick) => " c ",
                (Player::Sente, PieceKind::Hen) => " H ",
                (Player::Gote, PieceKind::Hen) => " h ",
            }
        };

        s.push_str(&format!(
            "後手持駒: ひよこ{}, きりん{}, ぞう{}\n",
            self.board.hands[1].chicks, self.board.hands[1].giraffes, self.board.hands[1].elephants
        ));
        s.push_str("  A  B  C\n");
        for y in 0..4 {
            s.push_str(&format!("{} ", y + 1));
            for x in 0..3 {
                let sq = y * 3 + x;
                let bit = 1 << sq;
                let mut found = false;
                for p in [Player::Sente, Player::Gote] {
                    for k in [
                        PieceKind::Lion,
                        PieceKind::Giraffe,
                        PieceKind::Elephant,
                        PieceKind::Chick,
                        PieceKind::Hen,
                    ] {
                        let idx = crate::board::get_piece_index(p, k);
                        if (self.board.piece_bbs[idx] & bit) != 0 {
                            s.push_str(piece_str(p, k));
                            found = true;
                            break;
                        }
                    }
                    if found {
                        break;
                    }
                }
                if !found {
                    s.push_str(" . ");
                }
            }
            s.push('\n');
        }
        s.push_str(&format!(
            "先手持駒: ひよこ{}, きりん{}, ぞう{}\n",
            self.board.hands[0].chicks, self.board.hands[0].giraffes, self.board.hands[0].elephants
        ));

        s
    }
}
