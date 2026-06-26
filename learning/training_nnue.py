import torch
import torch.nn as nn
import torch.optim as optim
from torch.utils.data import Dataset, DataLoader
import struct
import csv
import os

# --- 定数の定義 ---
FEATURE_SIZE = 132  # 盤上120 + 持ち駒12
HIDDEN_SIZE = 32    # アキュムレータ(第1隠れ層)のサイズ
QUANT_SCALE = 64.0  # Rust側と合わせる (2^6 = 64)

# --- 1. データセットの定義 ---
# CSVファイルから学習データを読み込み、PyTorchが扱えるテンソルに変換します
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
                # 最初のカラムはターゲットスコア (1.0, 0.0, -1.0)
                target = float(row[0])
                # 残りのカラムは特徴量IDのリスト
                features = [int(x) for x in row[1:] if x.strip()]
                self.data.append((target, features))
                
        print(f"合計 {len(self.data)} 局面のデータを読み込みました。")
                
    def __len__(self):
        return len(self.data)
        
    def __getitem__(self, idx):
        target, feature_indices = self.data[idx]
        
        # 132次元のゼロテンソル(0の配列)を作成
        feature_tensor = torch.zeros(FEATURE_SIZE, dtype=torch.float32)
        # 盤面に存在する駒のインデックスだけ '1' にする (One-hot / Sparse表現)
        if feature_indices:
            feature_tensor[feature_indices] = 1.0
            
        target_tensor = torch.tensor([target], dtype=torch.float32)
        return feature_tensor, target_tensor

# --- 2. ネットワーク構造の定義 ---
class AnimalShogiNNUE(nn.Module):
    def __init__(self):
        super(AnimalShogiNNUE, self).__init__()
        # 入力層 -> アキュムレータ
        self.feature_layer = nn.Linear(FEATURE_SIZE, HIDDEN_SIZE)
        # アキュムレータ -> 最終評価値
        self.output_layer = nn.Linear(HIDDEN_SIZE, 1)

    def forward(self, features):
        acc = self.feature_layer(features)
        
        acc = torch.clamp(acc, min=0.0, max=127.0 / QUANT_SCALE)
        score = self.output_layer(acc)
        return score

# --- 3. Rustエンジン向けに重みを量子化してエクスポート ---
def export_to_rust(model, filename):
    print("Rust用のバイナリ形式に変換・エクスポートしています...")
    feature_w = model.feature_layer.weight.detach().numpy()
    feature_b = model.feature_layer.bias.detach().numpy()
    output_w = model.output_layer.weight.detach().numpy()
    output_b = model.output_layer.bias.detach().numpy()

    with open(filename, "wb") as f:
        for feat_idx in range(FEATURE_SIZE):
            for hid_idx in range(HIDDEN_SIZE):
                w_i16 = int(round(feature_w[hid_idx, feat_idx] * QUANT_SCALE))
                f.write(struct.pack("<h", w_i16))
                
        for hid_idx in range(HIDDEN_SIZE):
            b_i16 = int(round(feature_b[hid_idx] * QUANT_SCALE))
            f.write(struct.pack("<h", b_i16))

        for hid_idx in range(HIDDEN_SIZE):
            w_i16 = int(round(output_w[0, hid_idx] * QUANT_SCALE))
            f.write(struct.pack("<h", w_i16))

        b_i32 = int(round(output_b[0] * QUANT_SCALE))
        f.write(struct.pack("<i", b_i32))
        
    print(f"完了しました！ '{filename}' をRustプロジェクトのルートに配置してください。")

# --- 4. メインの学習ループ ---
def train_model():
    # データセットとデータローダーの準備
    dataset = AnimalShogiDataset("../training_data.csv")
    
    # バッチサイズ (一度に学習する局面数)
    # データが少ない場合は小さく(32等)、多い場合は大きく(256等)します
    batch_size = min(256, len(dataset))
    dataloader = DataLoader(dataset, batch_size=batch_size, shuffle=True)
    
    model = AnimalShogiNNUE()
    criterion = nn.MSELoss()
    optimizer = optim.Adam(model.parameters(), lr=0.001)
    
    epochs = 50 # 全データを何周学習させるか
    
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
        
        # 10エポックごとに進捗を表示
        if epoch % 10 == 0 or epoch == 1:
            print(f"Epoch [{epoch}/{epochs}], Loss: {avg_loss:.6f}")
            
    print("--- 学習完了 ---")
    
    # エクスポート
    export_to_rust(model, "../nnue_weights.bin")

if __name__ == "__main__":
    train_model()