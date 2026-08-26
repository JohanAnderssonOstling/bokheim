#!/usr/bin/env bash
set -euo pipefail

app_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workspace_dir="$(cd "$app_dir/../.." && pwd)"
output_dir="$app_dir/dist"
local_bindgen="$workspace_dir/target/wasm-bindgen-cli/bin/wasm-bindgen"

cd "$app_dir"
RUSTC_BOOTSTRAP=1 cargo build -Z build-std=std,panic_abort --manifest-path "$workspace_dir/Cargo.toml" -p web-reader-smoke --target wasm32-unknown-unknown --release
mkdir -p "$output_dir/pkg"
if [[ -x "$local_bindgen" ]]; then
    wasm_bindgen="$local_bindgen"
else
    wasm_bindgen="$(command -v wasm-bindgen)"
fi
"$wasm_bindgen" --target web --out-dir "$output_dir/pkg" --out-name web_reader_smoke "$workspace_dir/target/wasm32-unknown-unknown/release/web_reader_smoke.wasm"
cp "$app_dir/index.html" "$output_dir/index.html"
