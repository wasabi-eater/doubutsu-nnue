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
use move_gen::{generate_moves, Move};
use nnue::NnueWeights;
use search::{search_best_move, SearchLimits};
use zobrist::{TranspositionTable, ZobristTable};

// --- ヘルパー関数: 指し手を人間が読める文字列に変換 ---
fn sq_to_string(sq: u8) -> String {
    let col = (b'A' + (sq % 3)) as char;
    let row = (b'1' + (sq / 3)) as char;
    format!("{}{}", col, row)
}

fn move_to_string(m: Move) -> String {
    // 16ビットにパックされた情報を手動で抽出
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
        // ブラウザ向けにメモリを節約 (約100万エントリ -> 50万エントリ程度)
        let tt = TranspositionTable::new(1024 * 512);

        // ★ コンパイル時に重みファイルをWASMバイナリに直接埋め込む魔法のマクロ ★
        // nnue_weights.bin はプロジェクトのルート (srcの外) に配置してください
        let weight_bytes = include_bytes!("../nnue_weights.bin");
        let weights = NnueWeights::load_from_slice(weight_bytes)
            .unwrap_or_else(|_| NnueWeights::new_dummy());

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
        self.tt = TranspositionTable::new(1024 * 512); // 置換表もクリア
    }

        // --- 🤖 AIの行動 ---
    pub fn search_and_apply_move(&mut self, time_limit_ms: u64) -> String {
        // 事前に合法手があるかチェックし、0手(詰み)なら不正な手を指さずに投了する
        let mut moves = Vec::new();
        generate_moves(&self.board, &mut moves);
        if moves.is_empty() {
            return "投了".to_string();
        }

        let limits = SearchLimits {
            max_time: Duration::from_millis(time_limit_ms),
            max_depth: 32, // 深さは実質無制限にして時間で区切る
        };

        let current_hash = self.board.compute_initial_hash(&self.z_table);
        let best_move = search_best_move(
            &self.board,
            &self.z_table,
            &mut self.tt,
            &self.weights,
            &limits,
            &self.history,
        );

        // 履歴を更新して手を適用
        self.history.push((current_hash, self.board.clone()));
        self.board.make_move(best_move, &self.z_table, current_hash);

        move_to_string(best_move)
    }


    // --- 👤 人間の行動 ---
    // UIからの入力 (盤上の駒を移動する) を適用する
    // from_sq, to_sq は 0~11 のインデックス (左上A1が0)
    pub fn apply_human_move(&mut self, from_sq: u8, to_sq: u8) -> bool {
        let mut moves = Vec::new();
        generate_moves(&self.board, &mut moves);

        // 入力されたマスと一致する合法手を探す
        for m in moves {
            let m_from = (m.0 & 0x0F) as u8;
            let m_to = ((m.0 >> 4) & 0x0F) as u8;
            let is_drop = (m.0 & (1 << 12)) != 0;

            if !is_drop && m_from == from_sq && m_to == to_sq {
                let current_hash = self.board.compute_initial_hash(&self.z_table);
                self.history.push((current_hash, self.board.clone()));
                self.board.make_move(m, &self.z_table, current_hash);
                return true; // 適用成功
            }
        }
        false // 非合法手
    }

    // UIからの入力 (持ち駒を打つ) を適用する
    // kind_val: 0:Lion, 1:Giraffe, 2:Elephant, 3:Chick, 4:Hen
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
                return true; // 適用成功
            }
        }
        false // 非合法手
    }

    // --- 🎮 状態の取得 ---
    // 勝敗状態を取得する。 -1: 進行中, 0: 引き分け(千日手), 1: 先手勝ち, 2: 後手勝ち
    pub fn get_winner(&self) -> i32 {
        if let Some(winner) = self.board.winner() {
            if winner == Player::Sente { 1 } else { 2 }
        } else {
            // 千日手判定
            let current_hash = self.board.compute_initial_hash(&self.z_table);
            let count = self
                .history
                .iter()
                .filter(|&&(h, ref b)| h == current_hash && *b == self.board)
                .count();
            if count >= 2 {
                return 0; // 引き分け
            }

            // ★ 修正: 合法手がゼロ（ステイルメイト・詰み）なら手番側の負け！
            let mut moves = Vec::new();
            generate_moves(&self.board, &mut moves);
            if moves.is_empty() {
                if self.board.side_to_move == Player::Sente {
                    return 2; // 先手が動けない -> 後手(AI)の勝ち
                } else {
                    return 1; // 後手が動けない -> 先手(あなた)の勝ち
                }
            }

            -1 // まだ勝負がついていない
        }
    }

    // 現在の手番が先手(1)か後手(2)かを返す
    pub fn get_turn(&self) -> i32 {
        if self.board.side_to_move == Player::Sente {
            1
        } else {
            2
        }
    }

    // JS側で盤面を描画・デバッグするために、盤面をアスキーアートの文字列として返す
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
            self.board.hands[1].chicks,
            self.board.hands[1].giraffes,
            self.board.hands[1].elephants
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
            self.board.hands[0].chicks,
            self.board.hands[0].giraffes,
            self.board.hands[0].elephants
        ));

        s
    }
}