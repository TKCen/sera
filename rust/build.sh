#!/bin/bash
# ─────────────────────────────────────────────────────────────────────────────
# sera Rust build script — smart cache management
#
# Usage: ./build.sh [debug|release]
#
# What it does:
#   debug:   Builds in debug mode with sccache. Cleans ONLY incremental/ on
#             first run (keeps deps/ cached). Subsequent runs skip dep rebuild
#             if Cargo.lock unchanged.
#   release: Builds release mode. No incremental cache in release.
#
# Environment:
#   RUSTC_WRAPPER=sccache  — use sccache for compiler caching
#   SCCACHE_DIR=~/.cache/sccache  — persistent cache across builds
#   SCCACHE_CACHE_SIZE=50G
# ─────────────────────────────────────────────────────────────────────────────

set -e

MODE="${1:-debug}"
CACHE_DIR="${SCCACHE_DIR:-$HOME/.cache/sccache}"
INCR_DIR="target/debug/incremental"

echo "[build] Mode: $MODE | sccache dir: $CACHE_DIR"

# Warm sccache on first build
if [ ! -d "$CACHE_DIR" ]; then
    echo "[build] First run — warming sccache cache..."
    mkdir -p "$CACHE_DIR"
fi

# For debug mode: if incremental is larger than 1GB, clean it selectively.
# This frees space while keeping deps/ (the actual compiled crates) intact.
if [ "$MODE" = "debug" ] && [ -d "$INCR_DIR" ]; then
    INCR_SIZE=$(du -sm "$INCR_DIR" 2>/dev/null | cut -f1)
    if [ "$INCR_SIZE" -gt 1000 ]; then
        echo "[build] incremental/ is ${INCR_SIZE}MB — cleaning selectively..."
        rm -rf "$INCR_DIR"
        echo "[build] incremental/ cleaned. deps/ kept warm in sccache."
    fi
fi

# Set sccache env
export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
export SCCACHE_DIR="$CACHE_DIR"
export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-50G}"

case "$MODE" in
    debug)
        echo "[build] Running: cargo build"
        cargo build
        ;;
    release)
        echo "[build] Running: cargo build --release"
        cargo build --release
        ;;
    *)
        echo "Usage: build.sh [debug|release]"
        exit 1
        ;;
esac

# Show cache stats after build
if command -v sccache &>/dev/null; then
    echo ""
    echo "[build] sccache stats:"
    sccache --show-stats 2>/dev/null | tail -5
fi
