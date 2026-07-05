use crate::board::{Board, PieceKind, Player, get_piece_index};
use crate::move_gen::Move;
// ★追加: アトミック操作のモジュール
use std::sync::atomic::{AtomicU64, Ordering};

struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }
    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }
}

pub struct ZobristTable {
    pub features: [u64; 134],
    pub side_to_move: u64,
}

impl ZobristTable {
    pub fn new() -> Self {
        let mut rng = XorShift64::new(0x123456789ABCDEF0);
        let mut table = Self {
            features: [0; 134],
            side_to_move: rng.next(),
        };
        for i in 0..134 {
            table.features[i] = rng.next();
        }
        table
    }
}

#[derive(Clone, Copy)]
pub struct TTEntry {
    pub key: u64,
    pub depth: u8,
    pub score: i32,
    pub best_move: Move,
    pub node_type: u8,
}

pub struct TTEntryAtomic {
    key_xor_data: AtomicU64,
    data: AtomicU64,
}

impl TTEntryAtomic {
    fn new() -> Self {
        Self {
            key_xor_data: AtomicU64::new(0),
            data: AtomicU64::new(0),
        }
    }

    fn pack(entry: &TTEntry) -> u64 {
        let mut d: u64 = 0;
        d |= (entry.score as i16 as u16 as u64) & 0xFFFF;
        d |= ((entry.best_move.0 as u64) & 0xFFFF) << 16;
        d |= ((entry.depth as u64) & 0xFF) << 32;
        d |= ((entry.node_type as u64) & 0xFF) << 40;
        d
    }

    fn unpack(key: u64, d: u64) -> TTEntry {
        let score = (d & 0xFFFF) as i16 as i32;
        let best_move = Move(((d >> 16) & 0xFFFF) as u16);
        let depth = ((d >> 32) & 0xFF) as u8;
        let node_type = ((d >> 40) & 0xFF) as u8;
        TTEntry {
            key,
            depth,
            score,
            best_move,
            node_type,
        }
    }
}

pub struct TranspositionTable {
    entries: Vec<TTEntryAtomic>,
    mask: u64,
}

impl TranspositionTable {
    pub fn new(capacity: usize) -> Self {
        let size = capacity.next_power_of_two();
        let mask = (size - 1) as u64;

        let mut entries = Vec::with_capacity(size);
        for _ in 0..size {
            entries.push(TTEntryAtomic::new());
        }

        Self { entries, mask }
    }

    pub fn probe(&self, key: u64) -> Option<TTEntry> {
        let index = (key & self.mask) as usize;
        let entry = &self.entries[index];

        // Stockfish式トリック: data と key_xor_data を読み込み、XORして検証する
        // もし別スレッドが書き込み中の半端な状態を読んだ場合、検証に失敗するため安全
        let data = entry.data.load(Ordering::Relaxed);
        let key_xor_data = entry.key_xor_data.load(Ordering::Relaxed);

        if key_xor_data ^ data == key {
            Some(TTEntryAtomic::unpack(key, data))
        } else {
            None
        }
    }

    pub fn store(&self, entry: TTEntry) {
        let index = (entry.key & self.mask) as usize;
        let dest = &self.entries[index];

        // Stockfish式トリック: 上書き前に既存のデータをチェックし、深さが浅ければ上書き
        let old_data = dest.data.load(Ordering::Relaxed);
        let old_depth = ((old_data >> 32) & 0xFF) as u8;

        if entry.depth >= old_depth || entry.node_type == crate::search::EXACT {
            let new_data = TTEntryAtomic::pack(&entry);
            // key_xor_data に key ^ data を保存する
            dest.key_xor_data
                .store(entry.key ^ new_data, Ordering::Relaxed);
            dest.data.store(new_data, Ordering::Relaxed);
        }
    }
}

impl Board {
    pub fn compute_initial_hash(&self, z_table: &ZobristTable) -> u64 {
        let mut h = 0;
        let active_features = self.extract_all_features();
        for &feature_id in &active_features {
            h ^= z_table.features[feature_id];
        }
        if self.side_to_move == Player::Sente {
            h ^= z_table.side_to_move;
        }
        h
    }

    // 盤上のすべてのFeature IDを抽出するヘルパー (初期ハッシュ計算やNNUE初期化用)
    pub fn extract_all_features(&self) -> Vec<usize> {
        let mut features = Vec::new();
        // 盤上の駒
        for player in [Player::Sente, Player::Gote] {
            for kind in [
                PieceKind::Lion,
                PieceKind::Giraffe,
                PieceKind::Elephant,
                PieceKind::Chick,
                PieceKind::Hen,
            ] {
                let p_idx = get_piece_index(player, kind);
                let mut bb = self.piece_bbs[p_idx];
                while bb != 0 {
                    let sq = bb.trailing_zeros() as usize;
                    // make_move.rs で定義した関数を使ってFeature IDを取得
                    features.push(crate::make_move::get_board_feature(player, kind, sq));
                    bb &= bb - 1;
                }
            }
        }
        // 持ち駒の抽出
        for player in [Player::Sente, Player::Gote] {
            let p_idx = player as usize;
            let chicks = self.hands[p_idx].chicks;
            if chicks > 0 {
                features.push(crate::make_move::get_hand_feature(
                    player,
                    PieceKind::Chick,
                    chicks,
                ));
            }

            let giraffes = self.hands[p_idx].giraffes;
            if giraffes > 0 {
                features.push(crate::make_move::get_hand_feature(
                    player,
                    PieceKind::Giraffe,
                    giraffes,
                ));
            }

            let elephants = self.hands[p_idx].elephants;
            if elephants > 0 {
                features.push(crate::make_move::get_hand_feature(
                    player,
                    PieceKind::Elephant,
                    elephants,
                ));
            }
        }

        // ★追加: 現在の手番フラグを初期特徴量に追加する
        if self.side_to_move == Player::Sente {
            features.push(132);
        } else {
            features.push(133);
        }

        features
    }
}
