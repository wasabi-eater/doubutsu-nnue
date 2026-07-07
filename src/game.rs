use crate::{
    board::{Board, PieceKind, Player},
    move_gen::Move,
};

pub fn sq_to_string(sq: u8) -> String {
    let col = (b'A' + (sq % 3)) as char;
    let row = (b'1' + (sq / 3)) as char;
    format!("{}{}", col, row)
}

pub fn move_to_string(m: Move) -> String {
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

pub fn board_string(board: &Board) -> String {
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
        board.hands[1].chicks, board.hands[1].giraffes, board.hands[1].elephants
    ));
    s.push_str("  A  B  C\n");
    for y in 0..4 {
        s.push_str(&format!("{} ", y + 1));
        for x in 0..3 {
            let sq = y * 3 + x;
            let bit = 1 << sq;
            let mut found = false;
            for p in [Player::Sente, Player::Gote] {
                for k in PieceKind::ALL {
                    let idx = crate::board::get_piece_index(p, k);
                    if (board.piece_bbs[idx] & bit) != 0 {
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
        board.hands[0].chicks, board.hands[0].giraffes, board.hands[0].elephants
    ));

    s
}
