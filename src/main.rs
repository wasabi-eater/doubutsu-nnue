use std::env;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod board;
mod make_move;
mod move_gen;
mod nnue;
mod search;
mod zobrist;

use board::{Board, PieceKind, Player};
use move_gen::{Move, generate_moves};
use nnue::NnueWeights;
use search::{SearchLimits, search_best_move};
use zobrist::{TranspositionTable, ZobristTable};

struct PositionRecord {
    features: Vec<usize>,
    side_to_move: Player,
}

struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        Self { state: seed.max(1) }
    }
    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }
    fn next_usize(&mut self, max: usize) -> usize {
        (self.next() % (max as u64)) as usize
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut mode = 0;

    if args.iter().any(|arg| arg == "train") {
        mode = 1;
    } else {
        println!("=== 🦁 どうぶつ将棋AI 🐥 ===");
        println!("モードを選択してください:");
        println!("1: 学習データ生成 (AI同士の自動対局を100回行う)");
        println!("2: 対人戦 (あなた: 先手 🐥 vs AI: 後手 🐶)");
        println!("3: 対人戦 (AI: 先手 🐥 vs あなた: 後手 🐶)");
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() {
            mode = input.trim().parse().unwrap_or(1);
        }
    }

    let z_table = ZobristTable::new();
    let mut tt = TranspositionTable::new(1024 * 1024);
    let nnue_weights = NnueWeights::load_from_file("nnue_weights.bin").unwrap_or_else(|_| {
        if mode != 1 {
            println!("⚠️ 学習済みの重みが見つからないため、AIはランダムに動きます！");
        }
        NnueWeights::new_dummy()
    });

    match mode {
        1 => generate_training_data(&z_table, &mut tt, &nnue_weights),
        2 => play_vs_human(&z_table, &mut tt, &nnue_weights, Player::Sente),
        3 => play_vs_human(&z_table, &mut tt, &nnue_weights, Player::Gote),
        _ => println!("不正な入力です。終了します。"),
    }
}

// --- 🎮 対人戦モード ---
fn play_vs_human(
    z_table: &ZobristTable,
    tt: &mut TranspositionTable,
    weights: &NnueWeights,
    human_player: Player,
) {
    let mut board = Board::initial_position();
    let limits = SearchLimits {
        max_time: Duration::from_millis(1000),
        max_depth: 31,
    };

    println!("\n対局を開始します！");

    let mut turn_count = 1;
    let mut game_history: Vec<u64> = Vec::new(); // ★追加: 実際のゲーム履歴

    loop {
        println!("\n====================================");
        println!("手数: {}手目", turn_count);
        print_board(&board);

        let current_hash = board.compute_initial_hash(z_table);

        // ★追加: 実際の対局における千日手判定 (3回現れたら引き分け)
        let count = game_history.iter().filter(|&&h| h == current_hash).count();
        if count >= 2 {
            // 今回で3回目
            println!("\n====================================");
            println!("千日手が成立しました。引き分けです！");
            break;
        }

        let mut moves = Vec::new();
        generate_moves(&board, &mut moves);
        if moves.is_empty() {
            println!(
                "合法手がありません。{} の負けです。",
                if board.side_to_move == Player::Sente {
                    "先手"
                } else {
                    "後手"
                }
            );
            break;
        }

        let best_move = if board.side_to_move == human_player {
            println!("\nあなたの番です。指し手を番号で選んでください:");
            for (i, &m) in moves.iter().enumerate() {
                println!("{:2}: {}", i, move_to_string(m));
            }
            loop {
                print!("> ");
                io::stdout().flush().unwrap();
                let mut input = String::new();
                io::stdin().read_line(&mut input).unwrap();
                if let Ok(choice) = input.trim().parse::<usize>()
                    && choice < moves.len()
                {
                    break moves[choice];
                }
                println!("正しい番号を入力してください。");
            }
        } else {
            println!("\nAIが思考中...");
            // ★追加: 探索関数に履歴を渡す
            search_best_move(&board, z_table, tt, weights, &limits, &game_history)
        };

        if board.side_to_move != human_player {
            println!("🤖 AIの指し手: {}", move_to_string(best_move));
        } else {
            println!("👤 あなたの指し手: {}", move_to_string(best_move));
        }

        game_history.push(current_hash); // 履歴に追加
        board.make_move(best_move, z_table, current_hash);

        if let Some(w) = board.winner() {
            println!("\n====================================");
            println!("最終盤面:");
            print_board(&board);
            match w {
                Player::Sente => println!("🎉 先手の勝利です！"),
                Player::Gote => println!("🎉 後手の勝利です！"),
            }
            break;
        }

        turn_count += 1;
        if turn_count > 200 {
            println!("200手を超えました。引き分けです。");
            break;
        }
    }
}

// --- 💾 学習データ生成モード ---
fn generate_training_data(
    z_table: &ZobristTable,
    tt: &mut TranspositionTable,
    weights: &NnueWeights,
) {
    let limits = SearchLimits {
        max_time: Duration::from_millis(50),
        max_depth: 11,
    };
    let mut rng = XorShift64::new();

    println!("学習データの生成を開始します...");

    for game_id in 1..=100 {
        let mut board = Board::initial_position();
        let mut turn_count = 1;
        let mut winner = None;
        let mut game_records: Vec<PositionRecord> = Vec::new();
        let mut game_history: Vec<u64> = Vec::new(); // ★追加: 履歴

        let random_plies = rng.next_usize(3) + 1;

        loop {
            game_records.push(PositionRecord {
                features: board.extract_all_features(),
                side_to_move: board.side_to_move,
            });

            let current_hash = board.compute_initial_hash(z_table);

            // ★追加: 千日手判定
            let count = game_history.iter().filter(|&&h| h == current_hash).count();
            if count >= 2 {
                winner = None;
                break;
            }

            let mut moves = Vec::new();
            generate_moves(&board, &mut moves);
            if moves.is_empty() {
                winner = Some(board.side_to_move.opponent());
                break;
            }

            let best_move = if turn_count <= random_plies {
                let random_idx = rng.next_usize(moves.len());
                moves[random_idx]
            } else {
                // ★追加: 履歴を渡す
                search_best_move(&board, z_table, tt, weights, &limits, &game_history)
            };

            game_history.push(current_hash);
            board.make_move(best_move, z_table, current_hash);

            if let Some(w) = board.winner() {
                winner = Some(w);
                break;
            }

            turn_count += 1;
            if turn_count > 200 {
                winner = None;
                break;
            }
        }

        let result_str = match winner {
            Some(Player::Sente) => "先手勝利",
            Some(Player::Gote) => "後手勝利",
            None => "引き分け",
        };

        if game_id % 10 == 0 || game_id == 1 {
            println!("ゲーム {} 終了: {} ({}手)", game_id, result_str, turn_count);
        }

        let sente_score: f32 = match winner {
            Some(Player::Sente) => 1.0,
            Some(Player::Gote) => -1.0,
            None => 0.0,
        };

        save_training_data("training_data.csv", &game_records, sente_score);
    }
    println!("データの生成が完了しました！");
}

fn save_training_data(filename: &str, records: &[PositionRecord], sente_score: f32) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(filename)
        .expect("ファイルを開けませんでした");

    for record in records {
        let target_score = if record.side_to_move == Player::Sente {
            sente_score
        } else {
            -sente_score
        };

        // 1. オリジナル
        write_record(&mut file, target_score, &record.features);

        // 2. 左右反転
        let flipped: Vec<usize> = record
            .features
            .iter()
            .map(|&f| flip_horizontal(f))
            .collect();
        write_record(&mut file, target_score, &flipped);

        // 3. 180度回転 (手番も反転したとみなせる)
        let rotated: Vec<usize> = record.features.iter().map(|&f| rotate_180(f)).collect();
        write_record(&mut file, target_score, &rotated);

        // 4. 180度回転 + 左右反転
        let flipped_rotated: Vec<usize> = rotated.iter().map(|&f| flip_horizontal(f)).collect();
        write_record(&mut file, target_score, &flipped_rotated);
    }
}

// 1行書き込みヘルパー
fn write_record(file: &mut std::fs::File, target_score: f32, features: &[usize]) {
    let features_str: Vec<String> = features.iter().map(|f| f.to_string()).collect();
    let features_csv = features_str.join(",");
    writeln!(file, "{},{}", target_score, features_csv).unwrap();
}

// --- データの水増し (Data Augmentation) 用ヘルパー関数 ---
fn flip_horizontal(f: usize) -> usize {
    if f < 120 {
        let sq = f % 12;
        let x = sq % 3;
        let y = sq / 3;
        let new_sq = y * 3 + (2 - x);
        f - sq + new_sq
    } else {
        f // 持ち駒は左右反転しても変わらない
    }
}

fn rotate_180(f: usize) -> usize {
    if f < 120 {
        let sq = f % 12;
        let kind_idx = (f / 12) % 5;
        let player_idx = f / 60;

        let new_sq = 11 - sq;
        let new_player_idx = 1 - player_idx; // 先手と後手を入れ替え

        new_player_idx * 60 + kind_idx * 12 + new_sq
    } else {
        if f < 126 {
            f + 6 // 先手の持ち駒 -> 後手の持ち駒
        } else {
            f - 6 // 後手の持ち駒 -> 先手の持ち駒
        }
    }
}

// --- 📝 ヘルパー関数: マスや手を人が読める文字列に変換 ---
fn sq_to_string(sq: u8) -> String {
    let col = (b'A' + (sq % 3)) as char;
    let row = (b'1' + (sq / 3)) as char;
    format!("{}{}", col, row)
}

fn move_to_string(m: Move) -> String {
    let piece_str = match m.piece_kind() {
        PieceKind::Lion => "ライオン",
        PieceKind::Giraffe => "きりん",
        PieceKind::Elephant => "ぞう",
        PieceKind::Chick => "ひよこ",
        PieceKind::Hen => "にわとり",
    };

    if m.is_drop() {
        format!("{} に {} を打つ", sq_to_string(m.sq_to()), piece_str)
    } else {
        let prom = if m.is_promote() { "成" } else { "" };
        format!(
            "{} から {} へ移動 ({}{})",
            sq_to_string(m.sq_from()),
            sq_to_string(m.sq_to()),
            piece_str,
            prom
        )
    }
}

fn print_board(board: &crate::board::Board) {
    let piece_str = |p: crate::board::Player, k: crate::board::PieceKind| -> &'static str {
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

    println!(
        "後手持駒: ひよこ{}, きりん{}, ぞう{}",
        board.hands[1].chicks, board.hands[1].giraffes, board.hands[1].elephants
    );
    println!("  A  B  C");
    for y in 0..4 {
        print!("{} ", y + 1);
        for x in 0..3 {
            let sq = y * 3 + x;
            let bit = 1 << sq;
            let mut found = false;
            for p in [Player::Sente, Player::Gote] {
                for k in PieceKind::ALL {
                    let idx = crate::board::get_piece_index(p, k);
                    if (board.piece_bbs[idx] & bit) != 0 {
                        print!("{}", piece_str(p, k));
                        found = true;
                        break;
                    }
                }
                if found {
                    break;
                }
            }
            if !found {
                print!(" . ");
            }
        }
        println!();
    }
    println!(
        "先手持駒: ひよこ{}, きりん{}, ぞう{}",
        board.hands[0].chicks, board.hands[0].giraffes, board.hands[0].elephants
    );
}
