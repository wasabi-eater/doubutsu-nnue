import subprocess
import os
import time

class Engine:
    def __init__(self, name, directory):
        self.name = name
        # OSの違いを吸収して実行ファイル名を決定
        exe_name = "doubutsu-nnue.exe" if os.name == "nt" else "./doubutsu-nnue"
        
        print(f"[{name}] を起動中... (ディレクトリ: {directory})")
        
        if not os.path.exists(os.path.join(directory, exe_name)):
            raise FileNotFoundError(f"エラー: {directory} に実行ファイルが見つかりません！")

        self.proc = subprocess.Popen(
            [exe_name, "engine"], 
            cwd=directory,
            stdin=subprocess.PIPE, 
            stdout=subprocess.PIPE, 
            text=True
        )
        self.send("isready")
        while True:
            line = self.proc.stdout.readline().strip()
            if line == "readyok":
                break

    def send(self, cmd):
        self.proc.stdin.write(cmd + "\n")
        self.proc.stdin.flush()

    def get_bestmove(self, moves_str):
        self.send(f"position moves {moves_str}")
        self.send("go")
        while True:
            line = self.proc.stdout.readline().strip()
            if line.startswith("bestmove"):
                return line.split(" ", 1)[1] # "1234", "0 draw", "0 loss" などを返す

    def close(self):
        self.send("quit")
        self.proc.terminate()

def play_game(engine_sente, engine_gote):
    moves = []
    turn = 0
    
    while True:
        active_engine = engine_sente if turn % 2 == 0 else engine_gote
        moves_str = " ".join(moves)
        
        # 思考して結果を受け取る
        res = active_engine.get_bestmove(moves_str)
        
        if res.startswith("0"):
            if "draw" in res:
                return "Draw", len(moves)
            else:
                # 負けが確定した場合、直前に指した相手の勝利
                winner = engine_gote.name if turn % 2 == 0 else engine_sente.name
                return winner, len(moves)
                
        moves.append(res)
        turn += 1
        
        if turn > 200:
            return "Draw", 200

if __name__ == "__main__":
    print("====================================")
    print(" 🏆 最強AI決定戦 トーナメント 🏆")
    print("====================================\n")
    
    # ★対戦させる2つのエンジンのフォルダ名を指定
    DIR_A = "../engine_v2"
    DIR_B = "../engine_v3"
    
    try:
        engine_a = Engine("Version_1 (旧)", DIR_A)
        engine_b = Engine("Version_2 (新)", DIR_B)
    except Exception as e:
        print(e)
        exit(1)

    a_wins = 0
    b_wins = 0
    draws = 0
    
    GAMES = 100  # ★対戦回数

    print("\n--- 対局開始 ---")
    start_time = time.time()

    for i in range(GAMES):
        # 公平を期すため、先手と後手を交互に入れ替える
        if i % 2 == 0:
            winner, plies = play_game(engine_a, engine_b)
            sente, gote = engine_a.name, engine_b.name
        else:
            winner, plies = play_game(engine_b, engine_a)
            sente, gote = engine_b.name, engine_a.name

        if winner == engine_a.name:
            a_wins += 1
        elif winner == engine_b.name:
            b_wins += 1
        else:
            draws += 1

        print(f"Game {i+1:03d} | 先手: {sente} vs 後手: {gote} -> 結果: {winner} 勝利 ({plies}手)")

    elapsed = time.time() - start_time
    print("\n====================================")
    print(" 🎉 大会結果 🎉")
    print("====================================")
    print(f"対局数: {GAMES} (所要時間: {elapsed:.1f}秒)")
    print(f" - {engine_a.name} の勝利数 : {a_wins}")
    print(f" - {engine_b.name} の勝利数 : {b_wins}")
    print(f" - 引き分け回数    : {draws}")
    
    if a_wins > b_wins:
        print(f"\n👑 優勝: {engine_a.name} !!")
    elif b_wins > a_wins:
        print(f"\n👑 優勝: {engine_b.name} !!")
    else:
        print("\n🤝 完全に互角です！")

    engine_a.close()
    engine_b.close()