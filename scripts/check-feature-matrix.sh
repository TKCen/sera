#!/usr/bin/env bash
# M4 feature-matrix gate — exits non-zero on any failure.
# Checks three configurations: default features, no-default-features, enterprise per-crate.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUST_DIR="$SCRIPT_DIR/../rust"

echo "=== [1/3] cargo check --workspace (default features) ==="
cargo check --workspace --manifest-path "$RUST_DIR/Cargo.toml"

echo "=== [2/3] cargo check --workspace --no-default-features ==="
cargo check --workspace --no-default-features --manifest-path "$RUST_DIR/Cargo.toml"

echo "=== [3/3] cargo check per-crate --features enterprise ==="
ENTERPRISE_CRATES=(sera-auth sera-gateway)
for crate in "${ENTERPRISE_CRATES[@]}"; do
    echo "  checking $crate --features enterprise"
    cargo check -p "$crate" --features enterprise --manifest-path "$RUST_DIR/Cargo.toml"
done

echo ""
echo "Feature matrix OK — all three configurations green."
