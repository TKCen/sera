#!/usr/bin/env bash
# verify-scenarios.sh — run the SERA test scenario battery.
#
# One-shot wrapper around the `sera-e2e-harness --features integration`
# tests.  Defaults to the mock LLM; point at a real provider (LM Studio,
# OpenAI-compatible gateway) by exporting:
#
#   SERA_E2E_LLM_BASE_URL=http://localhost:1234/v1
#   SERA_E2E_MODEL=lmstudio-community/meta-llama-3-8b
#
# Usage:
#   ./scripts/verify-scenarios.sh           # all phases
#   ./scripts/verify-scenarios.sh s1        # one scenario group (s1_bootstrap)
#   ./scripts/verify-scenarios.sh s3        # single-agent smoketest group
#
# Run single-threaded because each scenario boots its own gateway on an
# ephemeral port — running them in parallel overwhelms rust-analyzer on
# memory-constrained dev boxes and occasionally trips the port-pick race
# (documented in harness `pick_free_port`).

set -euo pipefail

# Resolve repo root (this script lives in <root>/scripts/).
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT/rust"

FILTER="${1:-}"

CARGO_ARGS=(
    test
    -p sera-e2e-harness
    --features integration
)

# Per-scenario filter: narrow cargo's test binary selection so only the
# requested group spins a gateway.
case "$FILTER" in
    "")          ;;
    s1)          CARGO_ARGS+=(--test scenarios_s1_bootstrap) ;;
    s2)          CARGO_ARGS+=(--test scenarios_s2_config) ;;
    s3)          CARGO_ARGS+=(--test scenarios_s3_single_agent) ;;
    s4)          CARGO_ARGS+=(--test scenarios_s4_policy) ;;
    s7)          CARGO_ARGS+=(--test scenarios_s7_workflow) ;;
    original)    CARGO_ARGS+=(--test local_profile_turn) ;;
    *)
        echo "unknown filter: $FILTER" >&2
        echo "known: s1 | s2 | s3 | s4 | s7 | original | <empty for all>" >&2
        exit 2
        ;;
esac

# `--test-threads=1` forces serial execution so the gateway boot logs are
# readable and ephemeral-port assignment is contention-free.
exec cargo "${CARGO_ARGS[@]}" -- --test-threads=1 --nocapture
