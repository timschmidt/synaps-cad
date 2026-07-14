#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${repo_root}"

wasm_bindgen_version="$(awk '
  $0 == "name = \"wasm-bindgen\"" { found = 1; next }
  found && $1 == "version" {
    gsub(/"/, "", $3)
    print $3
    exit
  }
' Cargo.lock)"
test -n "${wasm_bindgen_version}"

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "wasm-bindgen ${wasm_bindgen_version} is required" >&2
  echo "install it with: cargo install wasm-bindgen-cli --version ${wasm_bindgen_version} --locked" >&2
  exit 1
fi

installed_version="$(wasm-bindgen --version | awk '{ print $2 }')"
if [[ "${installed_version}" != "${wasm_bindgen_version}" ]]; then
  echo "wasm-bindgen ${wasm_bindgen_version} is required; found ${installed_version}" >&2
  exit 1
fi

cargo build --locked --release --target wasm32-unknown-unknown

mkdir -p dist/pkg
wasm-bindgen \
  --target web \
  --out-dir dist/pkg \
  --out-name synaps_cad \
  target/wasm32-unknown-unknown/release/synaps-cad.wasm
cp web/index.html dist/index.html
touch dist/.nojekyll

test -s dist/index.html
test -s dist/pkg/synaps_cad.js
test -s dist/pkg/synaps_cad_bg.wasm
grep -Fq 'from "./pkg/synaps_cad.js"' dist/index.html
