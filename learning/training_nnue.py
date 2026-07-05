import torch
import torch.nn as nn
import torch.optim as optim
from torch.utils.data import Dataset, DataLoader
import struct
import csv
import os

# --- 定数の定義 ---
FEATURE_SIZE = 134
HIDDEN_SIZE = 128
QUANT_SCALE = 128.0  # 2^7 = 128 (Rust側と一致)

# 1.0(勝ち) を 600点 として学習させる。これによりRust側の整数探索で十分な解像度が得られる
SCORE_SCALE = 600.0 

# --- 1. データセットの定義 ---
class AnimalShogiDataset(Dataset):
    def __init__(self, csv_file):
        self.data = []
        print(f"'{csv_file}' を読み込んでいます...")
        
        if not os.path.exists(csv_file):
            raise FileNotFoundError(f"{csv_file} が見つかりません。Rustで生成したか確認してください。")

        with open(csv_file, 'r') as f:
            reader = csv.reader(f)
            for row in reader:
                if not row:
                    continue
                # ★修正: ターゲットを SCORE_SCALE 倍に引き伸ばす
                target = float(row[0]) * SCORE_SCALE
                
                features = [int(x) for x in row[1:] if x.strip()]
                self.data.append((target, features))
                
        print(f"合計 {len(self.data)} 局面のデータを読み込みました。")
                
    def __len__(self):
        return len(self.data)
        
    def __getitem__(self, idx):
        target, feature_indices = self.data[idx]
        
        feature_tensor = torch.zeros(FEATURE_SIZE, dtype=torch.float32)
        if feature_indices:
            feature_tensor[feature_indices] = 1.0
            
        target_tensor = torch.tensor([target], dtype=torch.float32)
        return feature_tensor, target_tensor

# --- 2. ネットワーク構造の定義 ---
class AnimalShogiNNUE(nn.Module):
    def __init__(self):
        super(AnimalShogiNNUE, self).__init__()
        self.feature_layer = nn.Linear(FEATURE_SIZE, HIDDEN_SIZE)
        self.output_layer = nn.Linear(HIDDEN_SIZE, 1)

    def forward(self, features):
        acc = self.feature_layer(features)
        
        # ★修正: Rust側の clamp(0, 127) と厳密に一致させるため、127.0 / QUANT_SCALE を上限とする
        acc = torch.clamp(acc, min=0.0, max=127.0 / QUANT_SCALE)
        
        # ★修正: 2乗(SCReLU)をやめ、シンプルな Clipped ReLU に戻す
        score = self.output_layer(acc)
        return score

# --- 3. Rustエンジン向けに重みをエクスポート ---
def export_to_rust(model, filename):
    print("Rust用のバイナリ形式に変換・エクスポートしています...")
    feature_w = model.feature_layer.weight.detach().numpy()
    feature_b = model.feature_layer.bias.detach().numpy()
    output_w = model.output_layer.weight.detach().numpy()
    output_b = model.output_layer.bias.detach().numpy()

    with open(filename, "wb") as f:
        # 入力 -> 隠れ層 (i16)
        for feat_idx in range(FEATURE_SIZE):
            for hid_idx in range(HIDDEN_SIZE):
                w_i16 = int(round(feature_w[hid_idx, feat_idx] * QUANT_SCALE))
                f.write(struct.pack("<h", w_i16))
                
        for hid_idx in range(HIDDEN_SIZE):
            b_i16 = int(round(feature_b[hid_idx] * QUANT_SCALE))
            f.write(struct.pack("<h", b_i16))

        # SCORE_SCALE で値が大きくなっているため、i16ではオーバーフローする危険があるため
        for hid_idx in range(HIDDEN_SIZE):
            w_i32 = int(round(output_w[0, hid_idx] * QUANT_SCALE))
            f.write(struct.pack("<i", w_i32))

        # バイアスも i32
        b_i32 = int(round(output_b[0] * QUANT_SCALE))
        f.write(struct.pack("<i", b_i32))
        
    print(f"完了しました！ '{filename}' をRustプロジェクトのルートに配置してください。")

# --- 4. メインの学習ループ ---
def train_model():
    dataset = AnimalShogiDataset("training_data.csv")
    
    batch_size = min(256, len(dataset))
    dataloader = DataLoader(dataset, batch_size=batch_size, shuffle=True)
    
    model = AnimalShogiNNUE()
    criterion = nn.MSELoss()
    optimizer = optim.Adam(model.parameters(), lr=0.001)
    
    epochs = 50 
    
    print("\n--- 学習開始 ---")
    for epoch in range(1, epochs + 1):
        total_loss = 0.0
        for features, targets in dataloader:
            optimizer.zero_grad()
            outputs = model(features)
            loss = criterion(outputs, targets)
            loss.backward()
            optimizer.step()
            
            total_loss += loss.item()
            
        avg_loss = total_loss / len(dataloader)
        
        if epoch % 10 == 0 or epoch == 1:
            print(f"Epoch [{epoch}/{epochs}], Loss: {avg_loss:.6f}")
            
    print("--- 学習完了 ---")
    export_to_rust(model, "nnue_weights.bin")

if __name__ == "__main__":
    train_model()