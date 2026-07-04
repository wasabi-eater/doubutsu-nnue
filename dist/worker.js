import init, { initThreadPool, AnimalShogiWasm } from './dist/pkg/your_project_name.js';

let game = null;

self.onmessage = async (e) => {
    const msg = e.data;
    
    // 1. ワーカー内のWASMを初期化
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
    // 2. 盤面のリセット
    else if (msg.type === 'reset') {
        if (game) game.reset();
    }
    // 3. メインスレッド(画面側)で人間が指した手を、ワーカー内の盤面にも同期させる
    else if (msg.type === 'human_move') {
        if (game) game.apply_human_move(msg.from, msg.to);
    }
    else if (msg.type === 'human_drop') {
        if (game) game.apply_human_drop(msg.kind, msg.to);
    }
    // 4. 重いAIの探索を実行する
    else if (msg.type === 'search') {
        if (game) {
            // ここで1秒間スレッドがブロックされますが、Web Workerなので画面(UI)は全くフリーズしません！
            const jsonStr = game.search_and_apply_move(BigInt(msg.time_limit));
            self.postMessage({ type: 'search_result', data: jsonStr });
        }
    }
};
