// どうぶつ将棋の盤面表現 (ビットボード版)
use crate::move_gen::ATTACK_TABLE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Player {
    Sente, // 先手 (きつね側 / ライオンが下を向いている)
    Gote,  // 後手 (たぬき側 / ライオンが上を向いている)
}

impl Player {
    // 手番を交代する便利メソッド
    pub fn opponent(self) -> Self {
        match self {
            Player::Sente => Player::Gote,
            Player::Gote => Player::Sente,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    Lion,     // ライオン
    Giraffe,  // きりん
    Elephant, // ぞう
    Chick,    // ひよこ
    Hen,      // にわとり
}

impl From<u8> for PieceKind {
    fn from(value: u8) -> Self {
        assert!(value < 5);
        match value {
            0 => Self::Lion,
            1 => Self::Giraffe,
            2 => Self::Elephant,
            3 => Self::Chick,
            4 => Self::Hen,
            _ => panic!(),
        }
    }
}
impl PieceKind {
    pub fn piece_id(self) -> usize {
        match self {
            Self::Lion => 0,
            Self::Giraffe => 1,
            Self::Elephant => 2,
            Self::Chick => 3,
            Self::Hen => 4,
        }
    }
    pub const ALL: [Self; 5] = [
        Self::Lion,
        Self::Giraffe,
        Self::Elephant,
        Self::Chick,
        Self::Hen,
    ];
}

// 盤上の駒を表現（プレイヤー × 駒種）
// 10種類の組み合わせがあるため、ビットボードを10個の配列として保持します
pub const PIECE_TYPE_COUNT: usize = 10;

#[inline]
pub fn get_piece_index(player: Player, kind: PieceKind) -> usize {
    let p_idx = match player {
        Player::Sente => 0,
        Player::Gote => 1,
    };
    let k_idx = kind as usize;
    p_idx * 5 + k_idx
}

// 持ち駒の最大保持数（ぞう2、きりん2、ひよこ2）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hand {
    pub chicks: u8,
    pub giraffes: u8,
    pub elephants: u8,
}

// --- 盤面全体を表す構造体 ---
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board {
    // 盤上の駒の位置を保持するビットボード。各要素が u16
    // index 0..5: 先手のライオン, きりん, ぞう, ひよこ, にわとり
    // index 5..10: 後手のライオン, きりん, ぞう, ひよこ, にわとり
    pub piece_bbs: [u16; PIECE_TYPE_COUNT],

    // プレイヤーごとの持ち駒
    pub hands: [Hand; 2],

    // 現在の手番
    pub side_to_move: Player,
}

impl Board {
    pub fn initial_position() -> Self {
        Self {
            piece_bbs: [
                1 << 10,
                1 << 11,
                1 << 9,
                1 << 7,
                0,
                1 << 1,
                1 << 0,
                1 << 2,
                1 << 4,
                0,
            ],
            hands: [
                Hand {
                    chicks: 0,
                    giraffes: 0,
                    elephants: 0,
                },
                Hand {
                    chicks: 0,
                    giraffes: 0,
                    elephants: 0,
                },
            ],
            side_to_move: Player::Sente,
        }
    }

    // 指定したマスのビットを立てるヘルパー
    #[inline]
    pub fn square_bit(sq: usize) -> u16 {
        debug_assert!(sq < 12);
        1 << sq
    }

    // 特定のプレイヤーのすべての駒がどこにあるかを合成したビットボードを取得
    pub fn occupied_by(&self, player: Player) -> u16 {
        let start = match player {
            Player::Sente => 0,
            Player::Gote => 5,
        };
        let mut bb = 0u16;
        for i in start..(start + 5) {
            bb |= self.piece_bbs[i];
        }
        bb
    }

    // 盤上のすべての駒（先手・後手両方）の位置
    pub fn occupied(&self, _player: Player) -> u16 {
        self.occupied_by(Player::Sente) | self.occupied_by(Player::Gote)
    }

    pub fn any_attacker(&self, target_sq: u32, turn: Player) -> bool {
        let turn_usize = if turn == Player::Sente { 0 } else { 1 };
        (0..PIECE_TYPE_COUNT / 2).any(|kind_id| {
            let mut bb = if turn == Player::Sente {
                self.piece_bbs[kind_id]
            } else {
                self.piece_bbs[kind_id + PIECE_TYPE_COUNT / 2]
            };
            while bb > 0 {
                let sq = bb.trailing_zeros() as usize;
                if (ATTACK_TABLE[turn_usize][kind_id][sq] & (1 << target_sq)) != 0 {
                    return true;
                }
                bb &= bb - 1;
            }
            false
        })
    }

    pub fn winner(&self) -> Option<Player> {
        let sente_lion = PieceKind::Lion.piece_id();
        let gote_lion = PieceKind::Lion.piece_id() + PIECE_TYPE_COUNT / 2;

        if self.piece_bbs[sente_lion] == 0 {
            return Some(Player::Gote);
        }
        if self.piece_bbs[gote_lion] == 0 {
            return Some(Player::Sente);
        }

        let sente_lion_sq = self.piece_bbs[sente_lion].trailing_zeros();
        if sente_lion_sq < 3 && !self.any_attacker(sente_lion_sq, Player::Gote) {
            return Some(Player::Sente);
        }

        let gote_lion_sq = self.piece_bbs[gote_lion].trailing_zeros();
        if gote_lion_sq >= 12 - 3 && !self.any_attacker(gote_lion_sq, Player::Sente) {
            return Some(Player::Gote);
        }

        None
    }
}
