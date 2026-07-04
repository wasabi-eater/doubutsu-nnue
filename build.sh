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

echo "=== 3.5. ブラウザネイティブESMのパス修正 ==="

find pkg/snippets -type f -name "*.js" -exec sed -i "s|from '../../'|from '../../doubutsu_nnue.js'|g" {} +

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