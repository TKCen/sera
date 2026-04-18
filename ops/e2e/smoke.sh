#!/usr/bin/env bash
# ops/e2e/smoke.sh — E2E smoke test for sera-runtime + mock LM Studio
#
# SCOPE: Tests the runtime+mock path directly via cargo test.
# FIXME: Wire in the full sera-gateway path (requires sera.yaml scaffolding,
#        SQLite DB init, and runtime harness binary on PATH). Track in a follow-up
#        bead: "smoke.sh gateway integration" once sera-gateway binary is stable.
#
# Usage:
#   ops/e2e/smoke.sh
#   CARGO_PROFILE=release ops/e2e/smoke.sh
#
# Requirements:
#   - Rust toolchain (cargo) on PATH
#   - curl, python3 (for JSON check)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUST_DIR="${REPO_ROOT}/rust"
CARGO_PROFILE="${CARGO_PROFILE:-debug}"

log() { echo "[smoke] $*"; }

# ---------------------------------------------------------------------------
# Trap: clean up any background processes on exit
# ---------------------------------------------------------------------------
PIDS=()
cleanup() {
    for pid in "${PIDS[@]:-}"; do
        kill "$pid" 2>/dev/null || true
    done
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Step 1: Build sera-gateway and sera-runtime
# ---------------------------------------------------------------------------
log "Building sera-gateway and sera-runtime (profile=${CARGO_PROFILE})..."
if [[ "${CARGO_PROFILE}" == "release" ]]; then
    cargo build --release -p sera-gateway -p sera-runtime \
        --manifest-path "${RUST_DIR}/Cargo.toml"
else
    cargo build -p sera-gateway -p sera-runtime \
        --manifest-path "${RUST_DIR}/Cargo.toml"
fi
log "Build complete."

# ---------------------------------------------------------------------------
# Step 2: Run the mock fixture integration test (verifies mock + runtime client)
# ---------------------------------------------------------------------------
log "Running mock LM Studio integration tests..."
cargo test -p sera-runtime --test mock_lm_studio_test \
    --manifest-path "${RUST_DIR}/Cargo.toml" \
    -- --nocapture
log "Integration tests passed."

# ---------------------------------------------------------------------------
# Step 3: Verify test binary exits 0 (already checked above)
# ---------------------------------------------------------------------------
log "All smoke checks passed."
log ""
log "NOTE: Full gateway smoke test (POST :3001/api/chat) is out of scope for"
log "this cycle. See FIXME in this file for follow-up work."
