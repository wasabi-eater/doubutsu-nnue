use std::fs::File;
use std::io::{Read, Result};

// --- 定数の定義 ---
// train_nnue.py の定義と完全に一致させます
pub const FEATURE_SIZE: usize = 132;
pub const HIDDEN_SIZE: usize = 32;

// 量子化関連の定数
pub const WEIGHT_SCALE_BITS: i32 = 6; // 2^6 = 64
pub const ACTIVATION_MAX: i16 = 127;

// --- ネットワークの重み ---
pub struct NnueWeights {
    pub feature_weights: [[i16; HIDDEN_SIZE]; FEATURE_SIZE],
    pub feature_biases: [i16; HIDDEN_SIZE],
    pub output_weights: [i16; HIDDEN_SIZE],
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
                file.read_exact(&mut buf)?;
                weights.feature_weights[feat_idx][hid_idx] = i16::from_le_bytes(buf);
            }
        }

        // 2. Feature -> Accumulator バイアス (i16)
        for hid_idx in 0..HIDDEN_SIZE {
            let mut buf = [0u8; 2];
            file.read_exact(&mut buf)?;
            weights.feature_biases[hid_idx] = i16::from_le_bytes(buf);
        }

        // 3. Accumulator -> Output 重み (i16)
        for hid_idx in 0..HIDDEN_SIZE {
            let mut buf = [0u8; 2];
            file.read_exact(&mut buf)?;
            weights.output_weights[hid_idx] = i16::from_le_bytes(buf);
        }

        // 4. Accumulator -> Output バイアス (i32)
        let mut buf = [0u8; 4];
        file.read_exact(&mut buf)?;
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
    // 探索の開始時 (ルートノード) でゼロから計算する場合
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

    // ★ make_move で取得した FeatureUpdate を使った差分更新 ★
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
        let mut sum: i32 = weights.output_bias;
        
        for i in 0..HIDDEN_SIZE {
            // Clipped ReLU
            let activation = self.values[i].clamp(0, ACTIVATION_MAX) as i32;
            sum += activation * (weights.output_weights[i] as i32);
        }
        
        // スケールを戻す (ビットシフト)
        sum >>= WEIGHT_SCALE_BITS;
        
        sum
    }
}