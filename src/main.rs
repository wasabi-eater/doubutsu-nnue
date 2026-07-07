use std::env;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::time::Duration;

use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
use rayon::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use doubutsu_nnue::board::{Board, Player};
use doubutsu_nnue::game::{board_string, move_to_string};
use doubutsu_nnue::move_gen::{Move, generate_moves};
use doubutsu_nnue::nnue::NnueWeights;
use doubutsu_nnue::search::{SearchLimits, search_best_move};
use doubutsu_nnue::zobrist::{TranspositionTable, ZobristTable};

struct PositionRecord {
    features: Vec<usize>,
    side_to_move: Player,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut mode = 0;

    if args.iter().any(|arg| arg == "train") {
        mode = 1;
    } else if args.iter().any(|arg| arg == "engine") {
        mode = 5;
    } else {
        println!("=== 🦁 どうぶつ将棋AI 🐥 ===");
        println!("モードを選択してください:");
        println!("1: 学習データ生成 (AI同士の自動対局を100回並列で行う)");
        println!("2: 対人戦 (あなた: 先手 🐥 vs AI: 後手 🐶)");
        println!("3: 対人戦 (AI: 先手 🐥 vs あなた: 後手 🐶)");
        println!("4: AI同士の真剣勝負 (AI: 先手 🐥 vs AI: 後手 🐶)");
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() {
            mode = input.trim().parse().unwrap_or(1);
        }
    }

    let z_table = ZobristTable::new();

    let nnue_weights = NnueWeights::load_from_file("nnue_weights.bin").unwrap_or_else(|e| {
        if mode != 1 {
            println!("⚠️ 学習済みの重みの読み込みに失敗しました: {:?}", e);
            println!("⚠️ AIはランダム(ダミー)に動きます！");
        }
        NnueWeights::new_dummy()
    });

    match mode {
        1 => generate_training_data(&z_table, &nnue_weights),
        2 => {
            let mut tt = TranspositionTable::new(1024 * 1024);
            play_vs_human(&z_table, &mut tt, &nnue_weights, Player::Sente)
        }
        3 => {
            let mut tt = TranspositionTable::new(1024 * 1024);
            play_vs_human(&z_table, &mut tt, &nnue_weights, Player::Gote)
        }
        4 => {
            let mut tt = TranspositionTable::new(1024 * 1024);
            play_ai_vs_ai(&z_table, &mut tt, &nnue_weights)
        }
        5 => run_engine_mode(&z_table, &nnue_weights),
        _ => println!("不正な入力です。終了します。"),
    }
}

// --- 🎮 AI同士の真剣勝負モード ---
fn play_ai_vs_ai(z_table: &ZobristTable, tt: &mut TranspositionTable, weights: &NnueWeights) {
    let mut board = Board::initial_position();
    let limits = SearchLimits {
        max_time: Duration::from_millis(2000), // 1手2秒の全力探索
        max_depth: 64,                         // 深さ制限は事実上なし
    };

    println!("\n🔥 AI同士の真剣勝負を開始します！ 🔥");

    let mut turn_count = 1;
    let mut game_history: Vec<(u64, Board)> = Vec::new();

    loop {
        println!("\n====================================");
        println!(
            "手数: {}手目 ({})",
            turn_count,
            if board.side_to_move == Player::Sente {
                "先手"
            } else {
                "後手"
            }
        );
        print_board(&board);

        let current_hash = board.compute_initial_hash(z_table);

        let count = game_history
            .iter()
            .filter(|&&(h, ref b)| h == current_hash && *b == board)
            .count();
        if count >= 2 {
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

        println!("\n🤖 AIが思考中...");
        let best_move = search_best_move(&board, z_table, tt, weights, &limits, &game_history).0;

        println!("💡 AIの指し手: {}", move_to_string(best_move));

        game_history.push((current_hash, board.clone()));
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

// --- 🤖 外部ツール連携用(USI風)エンジンモード ---
fn run_engine_mode(z_table: &ZobristTable, weights: &NnueWeights) {
    let tt = TranspositionTable::new(1024 * 1024);
    let limits = SearchLimits {
        max_time: Duration::from_millis(500),
        max_depth: 64,
    };

    let mut board = Board::initial_position();
    let mut game_history: Vec<(u64, Board)> = Vec::new();

    loop {
        let mut input = String::new();
        if io::stdin().read_line(&mut input).unwrap_or(0) == 0 {
            break;
        }
        let input = input.trim();

        if input == "quit" {
            break;
        } else if input == "isready" {
            println!("readyok");
            io::stdout().flush().unwrap();
        } else if input.starts_with("position moves") {
            board = Board::initial_position();
            game_history.clear();
            let parts: Vec<&str> = input.split_whitespace().collect();
            for m_str in parts.iter().skip(2) {
                if let Ok(m_val) = m_str.parse::<u16>() {
                    let m = Move(m_val);
                    let hash = board.compute_initial_hash(z_table);
                    game_history.push((hash, board.clone()));
                    board.make_move(m, z_table, hash);
                }
            }
        } else if input == "go" {
            let current_hash = board.compute_initial_hash(z_table);
            let is_draw = game_history
                .iter()
                .filter(|&&(h, ref b)| h == current_hash && *b == board)
                .count()
                >= 2;

            let mut moves = Vec::new();
            generate_moves(&board, &mut moves);

            // 終局判定
            if is_draw || game_history.len() > 200 {
                println!("bestmove 0 draw");
            } else if board.winner().is_some() || moves.is_empty() {
                println!("bestmove 0 loss");
            } else {
                let (best_move, _) =
                    search_best_move(&board, z_table, &tt, weights, &limits, &game_history);
                println!("bestmove {}", best_move.0);
            }
            io::stdout().flush().unwrap();
        }
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
    let mut game_history: Vec<(u64, Board)> = Vec::new();

    loop {
        println!("\n====================================");
        println!("手数: {}手目", turn_count);
        print_board(&board);

        let current_hash = board.compute_initial_hash(z_table);

        let count = game_history
            .iter()
            .filter(|&&(h, ref b)| h == current_hash && *b == board)
            .count();
        if count >= 2 {
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
            search_best_move(&board, z_table, tt, weights, &limits, &game_history).0
        };

        if board.side_to_move != human_player {
            println!("🤖 AIの指し手: {}", move_to_string(best_move));
        } else {
            println!("👤 あなたの指し手: {}", move_to_string(best_move));
        }

        game_history.push((current_hash, board.clone()));
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

// --- 💾 学習データ生成モード (並列化対応) ---
fn generate_training_data(z_table: &ZobristTable, weights: &NnueWeights) {
    let num_games = 100;
    println!("学習データの生成を開始します (全 {} ゲーム)...", num_games);

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("training_data.csv")
        .expect("ファイルを開けませんでした");
    let file_mutex = Arc::new(Mutex::new(file));

    let completed_games = Arc::new(AtomicUsize::new(0));

    (1..=num_games).into_par_iter().for_each(|game_id| {
        let mut board = Board::initial_position();
        let mut turn_count = 1;
        let winner;
        let mut game_records: Vec<PositionRecord> = Vec::new();
        let mut game_history: Vec<(u64, Board)> = Vec::new();

        let tt = TranspositionTable::new(1024 * 512);

        let mut rng = SmallRng::seed_from_u64(game_id as u64);

        let random_plies = rng.random_range(0..3usize) + 1;

        let limits = SearchLimits {
            max_time: Duration::from_millis(50),
            max_depth: 11,
        };

        loop {
            game_records.push(PositionRecord {
                features: board.extract_all_features(),
                side_to_move: board.side_to_move,
            });

            let current_hash = board.compute_initial_hash(z_table);

            let count = game_history
                .iter()
                .filter(|&&(h, ref b)| h == current_hash && *b == board)
                .count();
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
                let random_idx = rng.random_range(0..moves.len());
                moves[random_idx]
            } else {
                search_best_move(&board, z_table, &tt, weights, &limits, &game_history).0
            };

            game_history.push((current_hash, board.clone()));
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

        let current_completed = completed_games.fetch_add(1, Ordering::SeqCst) + 1;
        if current_completed % 10 == 0 || current_completed == 1 {
            println!(
                "ゲーム終了: {} ({}手) [{}/{}]",
                result_str, turn_count, current_completed, num_games
            );
        }

        let sente_score: f32 = match winner {
            Some(Player::Sente) => 1.0,
            Some(Player::Gote) => -1.0,
            None => 0.0,
        };

        save_training_data_safe(&file_mutex, &game_records, sente_score);
    });

    println!("データの生成が完了しました！");
}

fn save_training_data_safe(
    file_mutex: &Arc<Mutex<std::fs::File>>,
    records: &[PositionRecord],
    sente_score: f32,
) {
    let mut file = file_mutex.lock().unwrap();

    for record in records {
        let target_score = sente_score;

        // 1. そのまま
        write_record(&mut file, target_score, &record.features);

        // 2. 左右反転 (スコアは変わらない)
        let flipped: Vec<usize> = record
            .features
            .iter()
            .map(|&f| flip_horizontal(f))
            .collect();
        write_record(&mut file, target_score, &flipped);

        // 3. 180度回転 (先手と後手の陣地が完全に入れ替わるため、スコアの符号を反転させる！)
        let rotated: Vec<usize> = record.features.iter().map(|&f| rotate_180(f)).collect();
        write_record(&mut file, -target_score, &rotated);

        // 4. 180度回転 ＋ 左右反転 (上記同様、スコアの符号を反転)
        let flipped_rotated: Vec<usize> = rotated.iter().map(|&f| flip_horizontal(f)).collect();
        write_record(&mut file, -target_score, &flipped_rotated);
    }
}

// 1行書き込みヘルパー
fn write_record(file: &mut std::fs::File, target_score: f32, features: &[usize]) {
    let features_str: Vec<String> = features.iter().map(|f| f.to_string()).collect();
    let features_csv = features_str.join(",");
    writeln!(file, "{},{}", target_score, features_csv).unwrap();
}

fn flip_horizontal(f: usize) -> usize {
    if f < 120 {
        let sq = f % 12;
        let x = sq % 3;
        let y = sq / 3;
        let new_sq = y * 3 + (2 - x);
        f - sq + new_sq
    } else {
        // 手番フラグ(132, 133)は左右反転しても変わらない
        f
    }
}

fn rotate_180(f: usize) -> usize {
    if f < 120 {
        let sq = f % 12;
        let kind_idx = (f / 12) % 5;
        let player_idx = f / 60;

        let new_sq = 11 - sq;
        let new_player_idx = 1 - player_idx;

        new_player_idx * 60 + kind_idx * 12 + new_sq
    } else if f < 132 {
        if f < 126 { f + 6 } else { f - 6 }
    } else {
        // 盤面を180度回転すると先手後手も入れ替わるため、手番フラグも反転させる
        if f == 132 { 133 } else { 132 }
    }
}

fn print_board(board: &Board) {
    println!("{}", board_string(board));
}
