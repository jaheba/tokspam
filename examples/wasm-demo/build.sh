#!/usr/bin/env sh
set -eu

cd "$(dirname "$0")"
cargo build --locked --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/tokspam_wasm_demo.wasm tokspam_demo.wasm
