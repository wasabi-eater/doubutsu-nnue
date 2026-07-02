use std::fs::File;
use std::io::{Read, Result, Cursor};

// --- 定数の定義 ---
pub const FEATURE_SIZE: usize = 132;
pub const HIDDEN_SIZE: usize = 128;

// ★ 修正: SCReLUに最適化したスケール定数
pub const WEIGHT_SCALE_BITS: i32 = 7; // 2^7 = 128
pub const ACTIVATION_MAX: i16 = 127;  // 127 を 1.0 とみなす

// --- ネットワークの重み ---
pub struct NnueWeights {
    pub feature_weights: [[i16; HIDDEN_SIZE]; FEATURE_SIZE],
    pub feature_biases: [i16; HIDDEN_SIZE],
    pub output_weights: [i32; HIDDEN_SIZE],
    pub output_bias: i32,
}

impl NnueWeights {
    // 重みファイルが見つからない時のためのダミー生成関数 (ゼロ初期化)
    pub fn new_dummy() -> Self {
        Self {
            feature_weights: [[0; HIDDEN_SIZE]; FEATURE_SIZE],
            feature_biases: [0; HIDDEN_SIZE],
            output_weights: [0; HIDDEN_SIZE],
            output_bias: 0,
        }
    }
    // Python側でエクスポートした nnue_weights.bin を読み込む関数
    pub fn load_from_file(path: &str) -> Result<Self> {
        let mut file = File::open(path)?;
        Self::load_from_reader(&mut file)    
    }

    // WASM向け: メモリ上のバイト配列からロードするメソッド
    pub fn load_from_slice(bytes: &[u8]) -> std::io::Result<Self> {
        let mut cursor = Cursor::new(bytes);
        Self::load_from_reader(&mut cursor)
    }

    pub fn load_from_reader<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut weights = NnueWeights {
            feature_weights: [[0; HIDDEN_SIZE]; FEATURE_SIZE],
            feature_biases: [0; HIDDEN_SIZE],
            output_weights: [0; HIDDEN_SIZE],
            output_bias: 0,
        };

        // 1. Feature -> Accumulator 重み (i16) を読み込む
        for feat_idx in 0..FEATURE_SIZE {
            for hid_idx in 0..HIDDEN_SIZE {
                let mut buf = [0u8; 2];
                reader.read_exact(&mut buf)?;
                weights.feature_weights[feat_idx][hid_idx] = i16::from_le_bytes(buf);
            }
        }

        // 2. Feature -> Accumulator バイアス (i16)
        for hid_idx in 0..HIDDEN_SIZE {
            let mut buf = [0u8; 2];
            reader.read_exact(&mut buf)?;
            weights.feature_biases[hid_idx] = i16::from_le_bytes(buf);
        }

        // 3. Accumulator -> Output 重み (i32に変更)
        for hid_idx in 0..HIDDEN_SIZE {
            let mut buf = [0u8; 4];
            reader.read_exact(&mut buf)?;
            weights.output_weights[hid_idx] = i32::from_le_bytes(buf);
        }

        // 4. Accumulator -> Output バイアス (i32)
        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf)?;
        weights.output_bias = i32::from_le_bytes(buf);

        Ok(weights)
    }
}

// --- アキュムレータ ---
#[derive(Clone)]
pub struct Accumulator {
    pub values: [i16; HIDDEN_SIZE],
}

impl Accumulator {
    pub fn refresh(weights: &NnueWeights, active_features: &[usize]) -> Self {
        let mut acc = Accumulator {
            values: weights.feature_biases,
        };
        for &feature_idx in active_features {
            for i in 0..HIDDEN_SIZE {
                acc.values[i] = acc.values[i].saturating_add(weights.feature_weights[feature_idx][i]);
            }
        }
        acc
    }

    pub fn update(&mut self, weights: &NnueWeights, added: &[usize], removed: &[usize]) {
        for &feature_idx in removed {
            for i in 0..HIDDEN_SIZE {
                self.values[i] = self.values[i].saturating_sub(weights.feature_weights[feature_idx][i]);
            }
        }
        for &feature_idx in added {
            for i in 0..HIDDEN_SIZE {
                self.values[i] = self.values[i].saturating_add(weights.feature_weights[feature_idx][i]);
            }
        }
    }

    // アキュムレータの値から最終的な評価値を計算
    pub fn evaluate(&self, weights: &NnueWeights) -> i32 {
        // オーバーフローを防ぐため、一時的に i64 で計算する
        let mut sum: i64 = 0;
        
        for i in 0..HIDDEN_SIZE {
            // 1. Clipped ReLU (0 ~ 127 に収める)
            let activation = self.values[i].clamp(0, ACTIVATION_MAX) as i64;
            
            // 2. SCReLU: 活性化値を2乗する
            let squared_activation = activation * activation;
            
            // 3. 重みと掛け合わせて足す
            sum += squared_activation * (weights.output_weights[i] as i64);
        }
        
        // この時点でスケールは (2^7)^2 * (2^7) = 2^21 倍に膨れ上がっている。
        // バイアスのスケール(2^7倍)と足し算できるように、一旦 2^14 (WEIGHT_SCALE_BITS * 2) で割る
        sum >>= WEIGHT_SCALE_BITS * 2;
        
        // スケールが 2^7 に揃ったので、ここで安全にバイアスを足す
        sum += weights.output_bias as i64;
        
        // 最後に残りの 2^7 で割って、元のスコアスケールに戻す
        (sum >> WEIGHT_SCALE_BITS) as i32
    }
}