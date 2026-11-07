#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ ! -f Cargo.toml ]]; then
  echo "No Cargo.toml yet: this is a specification-first starter. Run this after workspace bootstrap." >&2
  exit 2
fi
cargo fmt --all -- --check
cargo check --workspace
if command -v cargo-nextest >/dev/null 2>&1; then
  cargo nextest run --workspace
else
  cargo test --workspace
fi
