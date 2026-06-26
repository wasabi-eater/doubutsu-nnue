use crate::board::{Board, PieceKind, Player, get_piece_index};

// --- 指し手 (Move) のデータ構造 ---
// 1手の情報を16ビット整数にパックして極限まで軽量化します。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Move(pub u16);

impl Move {
    // 情報をビットシフトとOR演算でパックする
    pub fn new(from: u8, to: u8, piece: PieceKind, is_promote: bool, is_drop: bool) -> Self {
        let mut m = 0u16;
        m |= (from as u16) & 0x0F; // 0-3 bit: 移動元 (0-11, 打つ場合は15等)
        m |= ((to as u16) & 0x0F) << 4; // 4-7 bit: 移動先 (0-11)
        m |= ((piece as u16) & 0x07) << 8; // 8-10 bit: 駒の種類
        if is_promote {
            m |= 1 << 11;
        } // 11 bit: 成りフラグ
        if is_drop {
            m |= 1 << 12;
        } // 12 bit: 持ち駒からの打ち込みフラグ
        Move(m)
    }

    // パックされた情報を取り出すメソッド群
    pub fn sq_from(self) -> u8 {
        (self.0 & 0x0F) as u8
    }
    pub fn sq_to(self) -> u8 {
        ((self.0 >> 4) & 0x0F) as u8
    }
    pub fn piece_kind(self) -> PieceKind {
        PieceKind::from(((self.0 >> 8) & 0x07) as u8)
    }
    pub fn is_promote(self) -> bool {
        (self.0 >> 11) != 0
    }
    pub fn is_drop(self) -> bool {
        (self.0 >> 12) != 0
    }
}

// --- 利きテーブル (Lookup Table) ---
// 事前計算された「各マスにおける各駒の移動可能範囲（ビットボード）」
// [先手/後手][駒種][12マス] = 移動可能なマスのビット表現(u16)
pub const ATTACK_TABLE: [[[u16; 12]; 5]; 2] = initialize_attack_table();

const fn initialize_attack_table() -> [[[u16; 12]; 5]; 2] {
    let mut table = [[[0; 12]; 5]; 2];
    let mut sq = 0;

    // Rustの const fn を使ってコンパイル時にテーブルを生成します
    while sq < 12 {
        // --- 座標の計算 ---
        // マス(0-11)から、x座標(0-2)とy座標(0-3)を計算
        let x = sq % 3;
        let y = sq / 3;

        // 各方向への移動先のインデックス(存在しない場合は -1 的な処理)
        let up = if y > 0 { sq - 3 } else { 99 };
        let down = if y < 3 { sq + 3 } else { 99 };
        let left = if x > 0 { sq - 1 } else { 99 };
        let right = if x < 2 { sq + 1 } else { 99 };

        let up_left = if y > 0 && x > 0 { sq - 4 } else { 99 };
        let up_right = if y > 0 && x < 2 { sq - 2 } else { 99 };
        let down_left = if y < 3 && x > 0 { sq + 2 } else { 99 };
        let down_right = if y < 3 && x < 2 { sq + 4 } else { 99 };

        // ヘルパーマクロ的にビットを立てる
        const fn bit(s: usize) -> u16 {
            if s < 12 { 1 << s } else { 0 }
        }

        // ==========================================
        // 先手 (Sente) の利き (上に向かって進む)
        // ==========================================
        let sente_idx = 0;
        // ライオン (全方向)
        table[sente_idx][PieceKind::Lion as usize][sq] = bit(up)
            | bit(down)
            | bit(left)
            | bit(right)
            | bit(up_left)
            | bit(up_right)
            | bit(down_left)
            | bit(down_right);
        // きりん (十字)
        table[sente_idx][PieceKind::Giraffe as usize][sq] =
            bit(up) | bit(down) | bit(left) | bit(right);
        // ぞう (斜め)
        table[sente_idx][PieceKind::Elephant as usize][sq] =
            bit(up_left) | bit(up_right) | bit(down_left) | bit(down_right);
        // ひよこ (前のみ)
        table[sente_idx][PieceKind::Chick as usize][sq] = bit(up);
        // にわとり (斜め後ろ以外)
        table[sente_idx][PieceKind::Hen as usize][sq] =
            bit(up) | bit(down) | bit(left) | bit(right) | bit(up_left) | bit(up_right);

        // ==========================================
        // 後手 (Gote) の利き (下に向かって進む)
        // ==========================================
        let gote_idx = 1;
        // ライオン、きりん、ぞうは先手と同じ
        table[gote_idx][PieceKind::Lion as usize][sq] =
            table[sente_idx][PieceKind::Lion as usize][sq];
        table[gote_idx][PieceKind::Giraffe as usize][sq] =
            table[sente_idx][PieceKind::Giraffe as usize][sq];
        table[gote_idx][PieceKind::Elephant as usize][sq] =
            table[sente_idx][PieceKind::Elephant as usize][sq];
        // ひよこ (後手は下へ進む)
        table[gote_idx][PieceKind::Chick as usize][sq] = bit(down);
        // にわとり (斜め上以外)
        table[gote_idx][PieceKind::Hen as usize][sq] =
            bit(up) | bit(down) | bit(left) | bit(right) | bit(down_left) | bit(down_right);

        sq += 1;
    }
    table
}

// --- 合法手生成関数のコア ---
pub fn generate_moves(board: &Board, moves: &mut Vec<Move>) {
    let me = board.side_to_move;
    let my_pieces = board.occupied_by(me);
    let all_pieces = board.occupied(Player::Sente); // 全駒の位置

    // 味方の駒がいるマスには移動できないためのマスク
    let move_mask = !my_pieces;

    // 1. 盤上の駒の移動
    for kind in PieceKind::ALL {
        let p_idx = get_piece_index(me, kind);
        let mut bb = board.piece_bbs[p_idx]; // この種類の駒のビットボード

        // ビットが立っているマス(駒があるマス)を順に処理
        while bb != 0 {
            // 最下位の立っているビットのインデックスを取得 (Rustの組み込み関数で超高速)
            let from_sq = bb.trailing_zeros() as usize;

            // 利きテーブルから移動可能なマスのビットボードを一瞬で取得！
            let attacks = ATTACK_TABLE[me as usize][kind as usize][from_sq];

            // 味方の駒がいないマスだけを残す (AND演算1回)
            let mut valid_moves = attacks & move_mask;

            // 移動先の候補をループしてMoveオブジェクトを生成
            while valid_moves != 0 {
                let to_sq = valid_moves.trailing_zeros() as usize;

                // --- 成りの判定 ---
                // 先手なら0〜2行目、後手なら9〜11行目が敵陣
                let is_promote_zone = match me {
                    Player::Sente => to_sq < 3,
                    Player::Gote => to_sq > 8,
                };

                // ひよこが敵陣に入れば強制的に成る
                let promote = kind == PieceKind::Chick && is_promote_zone;

                moves.push(Move::new(from_sq as u8, to_sq as u8, kind, promote, false));

                // 処理したマスを消す
                valid_moves &= valid_moves - 1;
            }

            // 処理した駒を消す
            bb &= bb - 1;
        }
    }

    // 2. 持ち駒を打つ手 (Drop) の生成
    // 空いているマスを全て取得
    let empty_squares = !all_pieces & 0x0FFF; // 下位12ビットだけ使う

    // 例: ひよこを持っているなら、空いているマスにひよこを打つ手を生成
    // (※ただし二歩などの禁手ルールはどうぶつ将棋にはありませんが、敵陣1段目への歩打ちは禁止などのルールをここでチェックします)
    // 今回は簡略化のため空きマスループの概念のみ記載
    
    if board.hands[me as usize].chicks > 0 {
        let mut target = empty_squares;
        while target != 0 {
            let to_sq = target.trailing_zeros() as usize;
            moves.push(Move::new(15, to_sq as u8, PieceKind::Chick, false, true));
            target &= target - 1;
        }
    }

    if board.hands[me as usize].elephants > 0 {
        let mut target = empty_squares;
        while target != 0 {
            let to_sq = target.trailing_zeros() as usize;
            moves.push(Move::new(15, to_sq as u8, PieceKind::Elephant, false, true));
            target &= target - 1;
        }
    }

    if board.hands[me as usize].giraffes > 0 {
        let mut target = empty_squares;
        while target != 0 {
            let to_sq = target.trailing_zeros() as usize;
            moves.push(Move::new(15, to_sq as u8, PieceKind::Giraffe, false, true));
            target &= target - 1;
        }
    }
}

