import subprocess
import os
import shutil
import time

# --- 設定 ---
# 1世代(データ生成＋学習)のサイクルを何回繰り返すか
ITERATIONS = 1000

# 過去のデータを蓄積するか、毎回リセットするか
# Trueにすると、前回のイテレーションのデータを消してから新しいデータを生成します。
# Falseにすると、どんどんデータが追記され巨大なデータセットになります。
CLEAR_DATA_EVERY_ITERATION = True 

DATA_FILE = "../training_data.csv"
WEIGHT_FILE = "../nnue_weights.bin"

def run_pipeline():
    print("=== どうぶつ将棋AI 自動学習パイプライン開始 ===")
    
    for i in range(1, ITERATIONS + 1):
        print(f"\n======================================")
        print(f" 世代 (Iteration): {i} / {ITERATIONS}")
        print(f"======================================")

        # 1. データのクリーンアップ (設定による)
        if CLEAR_DATA_EVERY_ITERATION and os.path.exists(DATA_FILE):
            print(f"過去の {DATA_FILE} を削除しています...")
            os.remove(DATA_FILE)

        # 2. Rust側で自己対局によるデータ生成
        # 探索エンジンは非常に重いため、必ず '--release' フラグをつけて最適化ビルドで実行します
        print(f"\n[1/2] 自己対局による学習データ生成を開始します...")
        start_time = time.time()
        
        # ※ "cargo run --release" を実行
        result = subprocess.run(["cargo", "run", "--release", "--", "train"], text=True, cwd="..")
        
        if result.returncode != 0:
            print("❌ Rustエンジンの実行中にエラーが発生しました。パイプラインを停止します。")
            break
            
        elapsed = time.time() - start_time
        print(f"データ生成完了 (所要時間: {elapsed:.1f}秒)")

        # 3. Python側で学習と nnue_weights.bin の更新
        print(f"\n[2/2] PyTorchによるNNUE学習と重み出力 (train_nnue.py)")
        
        result = subprocess.run(["uv", "run", "training_nnue.py"], text=True)
        
        if result.returncode != 0:
            print("❌ 学習スクリプトの実行中にエラーが発生しました。パイプラインを停止します。")
            break

        print(f"\n✅ 世代 {i} 完了！ 新しい {WEIGHT_FILE} が生成されました。")
        
        # 4. オプション: 世代ごとの重みファイルをバックアップしておく
        # 後で「第5世代と第10世代を戦わせる」といった検証に役立ちます
        backup_name = f"../checkpoints/nnue_weights_gen{i}.bin"
        shutil.copy(WEIGHT_FILE, backup_name)
        print(f"バックアップを保存しました: {backup_name}")

    print("\n=== すべての自動学習パイプラインが完了しました！ ===")

if __name__ == "__main__":
    run_pipeline()