#!/usr/bin/env bash
# Validation loop for agents and humans.
# Usage: bash scripts/validate.sh [workspace]
# Examples:
#   bash scripts/validate.sh          # validate all workspaces
#   bash scripts/validate.sh core     # validate core only
#   bash scripts/validate.sh web      # validate web only
#
# Works from any directory — detects project root via git.

set -euo pipefail

# Find project root (works in worktrees too)
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || echo "")"
if [ -z "$ROOT" ]; then
  echo "Error: not inside a git repository" >&2
  exit 1
fi

WORKSPACE="${1:-}"
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

pass() { echo -e "${GREEN}  ✓ $1${NC}"; }
fail() { echo -e "${RED}  ✗ $1${NC}"; return 1; }

ERRORS=0

run_step() {
  local step_name="$1"
  shift
  if "$@" > /dev/null 2>&1; then
    pass "$step_name"
  else
    fail "$step_name"
    ERRORS=$((ERRORS + 1))
    # Don't exit — run all steps so the agent sees every failure at once
  fi
}

echo "=== SERA Validation Loop ==="
echo -e "  Root: ${YELLOW}${ROOT}${NC}"
[ -n "$WORKSPACE" ] && echo -e "  Scope: ${YELLOW}${WORKSPACE}${NC}"
echo ""

# Step 1: Typecheck
echo "Step 1/3: Typecheck"
if [ -n "$WORKSPACE" ]; then
  run_step "typecheck:${WORKSPACE}" npm run "typecheck:${WORKSPACE}" --prefix "$ROOT"
else
  run_step "typecheck" npm run typecheck --prefix "$ROOT"
fi

# Step 2: Lint
echo "Step 2/3: Lint"
if [ -n "$WORKSPACE" ]; then
  run_step "lint:${WORKSPACE}" npm run "lint:${WORKSPACE}" --prefix "$ROOT"
else
  run_step "lint" npm run lint --prefix "$ROOT"
fi

# Step 3: Test
echo "Step 3/3: Test"
if [ -n "$WORKSPACE" ]; then
  run_step "test:${WORKSPACE}" npm run test --prefix "$ROOT" --workspace="$WORKSPACE"
else
  run_step "test" npm run test --prefix "$ROOT"
fi

echo ""
if [ "$ERRORS" -gt 0 ]; then
  echo -e "${RED}=== ${ERRORS} step(s) failed ===${NC}"
  echo ""
  echo "Fix the failures above and re-run: bash scripts/validate.sh${WORKSPACE:+ $WORKSPACE}"
  exit 1
else
  echo -e "${GREEN}=== All checks passed ===${NC}"
fi
