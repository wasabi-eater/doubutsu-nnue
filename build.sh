#!/bin/bash
set -e

echo "=== 1. Rust Nightlyのインストール ==="
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly
source "$HOME/.cargo/env"
rustup component add rust-src --toolchain nightly

echo "=== 2. wasm-packのインストール ==="
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh

echo "=== 3. WASMのマルチスレッドビルド実行 ==="
wasm-pack build --target web --release

echo "=== 3.5. ブラウザネイティブESMのパス修正 (Node.jsによる完全版) ==="

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
// ../../ という曖昧なパスを、確実に ../../doubutsu_nnue.js に書き換える
content = content.replace(/from\s+['"]../../['"]/g, "from '../../doubutsu_nnue.js'");
content = content.replace(/from\s+['"]../../doubutsu[_-]nnue['"]/g, "from '../../doubutsu_nnue.js'");
fs.writeFileSync(fullPath, content);
console.log('✅ パスを修正しました: ' + fullPath);
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