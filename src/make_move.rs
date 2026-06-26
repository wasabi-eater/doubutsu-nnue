// 先ほど定義した Board 構造体に対する実装 (impl Board) の続きです

use crate::board::{Board, Player, PieceKind, get_piece_index};
use crate::move_gen::Move;
use crate::zobrist::ZobristTable;

// --- NNUE特徴量インデックスの計算 ---
// どうぶつ将棋の特徴量は全部で132個で収まります
// 0..119: 盤上の駒 (プレイヤー2 × 駒種5 × マス12)
// 120..131: 持ち駒の状態 (プレイヤー2 × (ひよこ2 + きりん2 + ぞう2))

#[inline]
pub fn get_board_feature(player: Player, kind: PieceKind, sq: usize) -> usize {
    let p_idx = match player { Player::Sente => 0, Player::Gote => 1 };
    let k_idx = kind as usize;
    p_idx * 60 + k_idx * 12 + sq
}

#[inline]
pub fn get_hand_feature(player: Player, kind: PieceKind, count: u8) -> usize {
    debug_assert!(count > 0 && count <= 2);
    let p_offset = match player { Player::Sente => 0, Player::Gote => 6 };
    let k_offset = match kind {
        PieceKind::Chick => 0,
        PieceKind::Giraffe => 2,
        PieceKind::Elephant => 4,
        _ => unreachable!(),
    };
    // countが1なら+0, 2なら+1
    120 + p_offset + k_offset + (count as usize - 1)
}

// --- 差分更新の記録用構造体 ---
// 1手で変わる特徴量は最大でも4つ程度なので、固定長配列でヒープ確保を回避します
pub struct FeatureUpdate {
    pub added: [usize; 4],
    pub added_count: usize,
    pub removed: [usize; 4],
    pub removed_count: usize,
}

impl FeatureUpdate {
    pub fn new() -> Self {
        Self { added: [0; 4], added_count: 0, removed: [0; 4], removed_count: 0 }
    }
    #[inline]
    pub fn add(&mut self, feature: usize) {
        self.added[self.added_count] = feature;
        self.added_count += 1;
    }
    #[inline]
    pub fn remove(&mut self, feature: usize) {
        self.removed[self.removed_count] = feature;
        self.removed_count += 1;
    }
}

impl Board {
    // 手を適用して盤面を更新し、NNUE用の差分リストと新しいハッシュ値を返す
    pub fn make_move(&mut self, m: Move, z_table: &ZobristTable, current_hash: u64) -> (FeatureUpdate, u64) {
        let me = self.side_to_move;
        let opponent = me.opponent();
        
        let to_sq = m.sq_to() as usize;
        let to_bit = Board::square_bit(to_sq);
        let piece_kind = m.piece_kind();
        
        let mut update = FeatureUpdate::new();

        // 1. 持ち駒からの打ち込み (Drop) の場合
        if m.is_drop() {
            // 持ち駒の「前の状態」を消して「新しい状態」を足す処理
            let old_count = match piece_kind {
                PieceKind::Chick => self.hands[me as usize].chicks,
                PieceKind::Giraffe => self.hands[me as usize].giraffes,
                PieceKind::Elephant => self.hands[me as usize].elephants,
                _ => unreachable!(),
            };
            
            // 例: ひよこが2個から1個になる場合、「ひよこ2個」のIDを消し、「ひよこ1個」のIDを足す
            update.remove(get_hand_feature(me, piece_kind, old_count));
            if old_count - 1 > 0 {
                update.add(get_hand_feature(me, piece_kind, old_count - 1));
            }

            match piece_kind {
                PieceKind::Chick => self.hands[me as usize].chicks -= 1,
                PieceKind::Giraffe => self.hands[me as usize].giraffes -= 1,
                PieceKind::Elephant => self.hands[me as usize].elephants -= 1,
                _ => unreachable!(),
            }
            
            // 盤面に駒が現れる
            let p_idx = get_piece_index(me, piece_kind);
            self.piece_bbs[p_idx] |= to_bit;
            update.add(get_board_feature(me, piece_kind, to_sq));
            
        } else {
            // 2. 盤上の駒の移動の場合
            let from_sq = m.sq_from() as usize;
            let from_bit = Board::square_bit(from_sq);
            let p_idx = get_piece_index(me, piece_kind);

            // 移動元の駒を消す
            self.piece_bbs[p_idx] &= !from_bit;
            update.remove(get_board_feature(me, piece_kind, from_sq));

            // 移動先に敵の駒があるか（駒を取る処理）
            let opponent_occupied = self.occupied_by(opponent);
            if (opponent_occupied & to_bit) != 0 {
                // 取った駒の種類を特定する
                for kind in [PieceKind::Lion, PieceKind::Giraffe, PieceKind::Elephant, PieceKind::Chick, PieceKind::Hen] {
                    let opp_idx = get_piece_index(opponent, kind);
                    if (self.piece_bbs[opp_idx] & to_bit) != 0 {
                        // 敵の盤面から駒を消す
                        self.piece_bbs[opp_idx] &= !to_bit;
                        update.remove(get_board_feature(opponent, kind, to_sq));
                        
                        // 自分の持ち駒に加える (にわとりを取った場合はひよこに戻る)
                        let captured_kind = if kind == PieceKind::Hen { PieceKind::Chick } else { kind };
                        
                        let old_count = match captured_kind {
                            PieceKind::Chick => self.hands[me as usize].chicks,
                            PieceKind::Giraffe => self.hands[me as usize].giraffes,
                            PieceKind::Elephant => self.hands[me as usize].elephants,
                            PieceKind::Lion => 0,
                            _ => unreachable!(),
                        };

                        if captured_kind != PieceKind::Lion {
                            if old_count > 0 {
                                update.remove(get_hand_feature(me, captured_kind, old_count));
                            }
                            update.add(get_hand_feature(me, captured_kind, old_count + 1));
                            
                            match captured_kind {
                                PieceKind::Chick => self.hands[me as usize].chicks += 1,
                                PieceKind::Giraffe => self.hands[me as usize].giraffes += 1,
                                PieceKind::Elephant => self.hands[me as usize].elephants += 1,
                                _ => unreachable!(),
                            }
                        }
                        break;
                    }
                }
            }

            // 移動先に自分の駒を置く (成りの処理を含む)
            let place_kind = if m.is_promote() {
                PieceKind::Hen // ひよこが成ったらにわとり
            } else {
                piece_kind
            };
            
            let place_idx = get_piece_index(me, place_kind);
            self.piece_bbs[place_idx] |= to_bit;
            update.add(get_board_feature(me, place_kind, to_sq));
        }

        // 3. 手番を交代する
        self.side_to_move = opponent;
        
        // -------------------------------------------------------------
        // ★ Zobrist ハッシュの魔法の更新 ★
        // FeatureUpdateで作った「消えたID」「現れたID」のリストを
        // そのままハッシュキーの配列インデックスとして使い、XORするだけ！
        // -------------------------------------------------------------
        let mut next_hash = current_hash;
        
        // 消えた駒のハッシュを抜く (XOR)
        for i in 0..update.removed_count {
            next_hash ^= z_table.features[update.removed[i]];
        }
        // 現れた駒のハッシュを入れる (XOR)
        for i in 0..update.added_count {
            next_hash ^= z_table.features[update.added[i]];
        }
        // 手番の反転
        next_hash ^= z_table.side_to_move;

        // 差分リストと、更新された新しいハッシュ値を返す
        (update, next_hash)
    }
}

// --- 探索木での使い方イメージ ---
/*
fn alpha_beta(
    board: &Board, 
    depth: u8, 
    alpha: i32, 
    beta: i32, 
    current_acc: &Accumulator, 
    current_hash: u64,
    z_table: &ZobristTable,
    tt: &mut TranspositionTable
) -> i32 {
    if depth == 0 { return current_acc.evaluate(&nnue_weights); }
    
    // 1. 置換表 (TT) の確認
    if let Some(entry) = tt.probe(current_hash) {
        // もし十分な深さまで読まれていれば、再計算せずにそのままスコアを返す
        if entry.depth >= depth {
            return entry.score; // (※実際にはαβの境界チェックが必要です)
        }
    }
    
    let mut moves = Vec::new();
    generate_moves(board, &mut moves);
    
    let mut best_score = -INFINITY;
    
    for m in moves {
        // 現在の盤面をクローン
        let mut next_board = board.clone();
        
        // 盤面を進めつつ、NNUE用の差分と次のハッシュ値を受け取る！
        let (feature_update, next_hash) = next_board.make_move(m, z_table, current_hash);
        
        // NNUEアキュムレータの差分更新
        let mut next_acc = current_acc.clone();
        next_acc.update(&nnue_weights, 
            &feature_update.added[..feature_update.added_count], 
            &feature_update.removed[..feature_update.removed_count]
        );
        
        // 次の深さへ再帰
        let score = -alpha_beta(&next_board, depth - 1, -beta, -alpha, &next_acc, next_hash, z_table, tt);
        
        // ... アルファベータの更新処理 ...
    }
    
    // 探索が終わったら、結果を置換表 (TT) に保存する
    // tt.store(TTEntry { key: current_hash, score: best_score, ... });
    
    best_score
}
*/
