// Zobrist Hashingと置換表 (Transposition Table) の実装

use crate::board::{Board, PieceKind, Player, get_piece_index};
use crate::move_gen::Move;


// --- 簡易的な疑似乱数生成器 (Xorshift64) ---
struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self { state: if seed == 0 { 1 } else { seed } }
    }
    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }
}


// --- Zobrist 乱数テーブル ---
pub struct ZobristTable {
    // ★ NNUEのFeature ID (0〜131) と1対1対応する64ビット乱数テーブル ★
    // 0..119: 盤上の駒、120..131: 持ち駒の状態
    pub features: [u64; 132],
    // 手番が先手か後手か
    pub side_to_move: u64,
}

impl ZobristTable {
    // プログラム起動時に乱数で初期化する
    pub fn new() -> Self {
        let mut rng = XorShift64::new(0x123456789ABCDEF0);
        let mut table = Self {
            features: [0; 132],
            side_to_move: rng.next(),
        };
        for i in 0..132 {
            table.features[i] = rng.next();
        }
        table
    }
}

// --- 置換表 (Transposition Table) のエントリ ---
#[derive(Clone, Copy)]
pub struct TTEntry {
    pub key: u64,        // 衝突確認用のZobristハッシュキー
    pub depth: u8,       // 探索深さ
    pub score: i32,      // 評価値
    pub best_move: Move, // この局面での最善手 (Move Orderingに必須)
    pub node_type: u8,   // EXACT(正確な値), UPPER_BOUND(βカット), LOWER_BOUND(αカット) の種類
}

// 探索エンジンに持たせる巨大な配列
pub struct TranspositionTable {
    pub entries: Vec<TTEntry>, // 実際のサイズは 2のべき乗（例: 2^20 = 約100万エントリ）にします
    pub mask: u64,
}

impl TranspositionTable {
    pub fn new(capacity: usize) -> Self {
        let size = capacity.next_power_of_two();
        let mask = (size - 1) as u64;

        // ダミーの空エントリで配列を埋める
        let empty_entry = TTEntry {
            key: 0,
            depth: 0,
            score: 0,
            best_move: Move(0), // ダミーの手
            node_type: 0,
        };

        Self {
            entries: vec![empty_entry; size],
            mask,
        }
    }
    // 取得
    pub fn probe(&self, key: u64) -> Option<TTEntry> {
        let index = (key & self.mask) as usize;
        let entry = self.entries[index];
        if entry.key == key {
            Some(entry) // ハッシュが一致すればキャッシュヒット！
        } else {
            None
        }
    }
    // 保存 (深さが深いものを優先して上書きするなど、置換戦略がいくつかあります)
    pub fn store(&mut self, entry: TTEntry) {
        let index = (entry.key & self.mask) as usize;
        self.entries[index] = entry;
    }
}

// --- Board構造体の拡張 ---
// Boardの中に現在のハッシュ値を保持させます
impl Board {
    // 初期局面のハッシュ値をゼロから計算する (NNUEの初期計算と同じ要領です)
    pub fn compute_initial_hash(&self, z_table: &ZobristTable) -> u64 {
        let mut h = 0;

        // 盤面に存在するすべてのFeature IDを取得する関数(仮)を呼ぶ
        let active_features = self.extract_all_features();
        for &feature_id in &active_features {
            h ^= z_table.features[feature_id];
        }

        // 手番のXOR
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
            for kind in PieceKind::ALL {
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

        features
    }
}
