#!/usr/bin/env bash
# Builds the page on Vercel: the engine to WebAssembly, then the page,
# pointed at the published wind.
set -euo pipefail

# Vercel's build image ships Rust; elsewhere, install it.
export PATH="/rust/bin:$HOME/.cargo/bin:$PATH"
if ! command -v rustup >/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
rustup target add wasm32-unknown-unknown

bun run wasm
UPWIND_WIND_BASE="${UPWIND_WIND_BASE:-https://itshaddad.github.io/upwind}" bun scripts/build-web.ts
