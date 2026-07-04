// ★修正: パッケージ名を 'doubutsu_nnue.js' に変更
import init, { initThreadPool, AnimalShogiWasm } from './pkg/doubutsu_nnue.js';

let game = null;

self.onmessage = async (e) => {
    const msg = e.data;
    
    if (msg.type === 'init') {
        try {
            await init();
            const hardwareConcurrency = navigator.hardwareConcurrency || 4;
            await initThreadPool(hardwareConcurrency);
            
            game = new AnimalShogiWasm();
            self.postMessage({ type: 'ready' });
        } catch (err) {
            console.error("Worker内でのWASM初期化に失敗しました", err);
        }
    }
    else if (msg.type === 'reset') {
        if (game) game.reset();
    }
    else if (msg.type === 'human_move') {
        if (game) game.apply_human_move(msg.from, msg.to);
    }
    else if (msg.type === 'human_drop') {
        if (game) game.apply_human_drop(msg.kind, msg.to);
    }
    else if (msg.type === 'search') {
        if (game) {
            const jsonStr = game.search_and_apply_move(BigInt(msg.time_limit));
            self.postMessage({ type: 'search_result', data: jsonStr });
        }
    }
};