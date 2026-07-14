use crate::{
    board::{Board, PieceKind, Player},
    make_move::FeatureUpdate,
    move_gen::{self, Move},
    zobrist::ZobristTable,
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

pub struct GameManager<'zt> {
    current_hash: u64,
    z_table: &'zt ZobristTable,
    board: Board,
    history: Vec<(u64, Board)>,
    winner: Option<Player>,
    side_to_move: Option<Player>, // 終局時はNone
    move_count: usize,
    moves: Vec<Move>,
}
impl<'zt> GameManager<'zt> {
    pub fn new(z_table: &'zt ZobristTable) -> Self {
        let board = Board::initial_position();
        let mut moves = Vec::new();
        move_gen::generate_moves(&board, &mut moves);
        Self {
            current_hash: board.compute_initial_hash(z_table),
            z_table,
            board,
            moves,
            history: Vec::new(),
            winner: None,
            side_to_move: Some(Player::Sente),
            move_count: 0,
        }
    }
    pub fn board(&self) -> &Board {
        &self.board
    }
    pub fn history(&self) -> &[(u64, Board)] {
        &self.history
    }
    pub fn winner(&self) -> Option<Player> {
        self.winner
    }
    pub fn side_to_move(&self) -> Option<Player> {
        self.side_to_move
    }
    pub fn is_finished(&self) -> bool {
        self.side_to_move.is_none()
    }
    pub fn is_draw(&self) -> bool {
        self.is_finished() && self.winner.is_none()
    }
    pub fn move_count(&self) -> usize {
        self.move_count
    }
    pub fn z_table(&self) -> &'zt ZobristTable {
        self.z_table
    }
    pub fn current_hash(&self) -> u64 {
        self.current_hash
    }
    pub fn moves(&self) -> &[Move] {
        if self.is_finished() { &[] } else { &self.moves }
    }
    fn check_draw(&self) -> bool {
        self.move_count >= 200
            || self
                .history
                .iter()
                .filter(|&(hash, board)| *hash == self.current_hash && *board == self.board)
                .count()
                >= 2
    }
    pub fn make_move(&mut self, m: Move) -> Option<FeatureUpdate> {
        let turn = self.side_to_move?;
        self.history.push((self.current_hash, self.board.clone()));
        let (feature_update, next_hash) = self.board.make_move(m, self.z_table, self.current_hash);
        self.current_hash = next_hash;
        self.move_count += 1;

        self.moves = Vec::new();
        move_gen::generate_moves(&self.board, &mut self.moves);

        if self.check_draw() {
            self.side_to_move = None;
        } else if let Some(winner) = self.board.winner() {
            self.winner = Some(winner);
            self.side_to_move = None;
        } else if self.moves.is_empty() {
            self.winner = Some(turn);
            self.side_to_move = None;
        } else {
            self.side_to_move = Some(turn.opponent());
        }
        Some(feature_update)
    }
}
