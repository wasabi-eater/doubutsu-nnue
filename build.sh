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

echo "=== 4. 公開用フォルダ(dist)の準備 ==="
rm -rf dist
mkdir -p dist
cp index.html dist/
cp worker.js dist/
cp _headers dist/
cp -r pkg dist/

echo "=== すべてのビルドが完了しました！ ==="