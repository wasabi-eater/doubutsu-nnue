#!/bin/bash
set -e

echo "=== 1. wasm-packのインストール ==="
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh

echo "=== 2. WASMのビルド実行 ==="
wasm-pack build --target web --release

echo "=== 3.5. ブラウザネイティブESMのパス修正 (完全網羅版) ==="

cat << 'EOF' > fix_paths.js
const fs = require('fs');
const path = require('path');
function fixImports(dir) {
    if (!fs.existsSync(dir)) return;
    fs.readdirSync(dir).forEach(file => {
        const fullPath = path.join(dir, file);
        if (fs.statSync(fullPath).isDirectory()) {
            fixImports(fullPath);
        } else if (fullPath.endsWith('.js')) {
            let content = fs.readFileSync(fullPath, 'utf8');

            const patterns = [
                // 2階層上 (../../)
                { from: "from '../../'", to: "from '../../doubutsu_nnue.js'" },
                { from: 'from "../../"', to: 'from "../../doubutsu_nnue.js"' },
                { from: "import('../../')", to: "import('../../doubutsu_nnue.js')" },
                { from: 'import("../../")', to: 'import("../../doubutsu_nnue.js")' },
                { from: "import('../../doubutsu_nnue')", to: "import('../../doubutsu_nnue.js')" },
                
                // 3階層上 (../../..)
                { from: "from '../../..'", to: "from '../../../doubutsu_nnue.js'" },
                { from: 'from "../../.."', to: 'from "../../../doubutsu_nnue.js"' },
                { from: "from '../../../'", to: "from '../../../doubutsu_nnue.js'" },
                { from: 'from "../../../"', to: 'from "../../../doubutsu_nnue.js"' },
                { from: "import('../../..')", to: "import('../../../doubutsu_nnue.js')" },
                { from: 'import("../../..")', to: 'import("../../../doubutsu_nnue.js")' },
                { from: "import('../../../')", to: "import('../../../doubutsu_nnue.js')" },
                { from: 'import("../../../")', to: 'import("../../../doubutsu_nnue.js")' }
            ];

            let modified = false;
            for (const p of patterns) {
                if (content.includes(p.from)) {
                    content = content.replaceAll(p.from, p.to);
                    modified = true;
                }
            }
            
            if (modified) {
                fs.writeFileSync(fullPath, content);
                console.log('✅ パスを修正しました: ' + fullPath);
            }
        }
    });
}
fixImports('pkg/snippets');
EOF

node fix_paths.js
rm fix_paths.js

echo "=== 4. Tailwind CSS の静的ビルド ==="
npx --yes tailwindcss@3 -o style.css --content index.html

echo "=== 5. 公開用フォルダ(dist)の準備 ==="
rm -rf dist
mkdir -p dist
cp index.html dist/
cp worker.js dist/
cp _headers dist/
cp style.css dist/
cp -r pkg dist/

echo "=== すべてのビルドが完了しました！ ==="
