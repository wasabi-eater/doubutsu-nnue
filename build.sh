#!/bin/bash
set -e

echo "=== 1. Rust Stableのインストール ==="
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

echo "=== 2. wasm-packのインストール ==="
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh

echo "=== 3. WASMのビルド実行 (Stable) ==="
wasm-pack build --target web --release

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