#!/usr/bin/env bash
# Full CI check: format + validate. Run before every push.
# Usage: bash scripts/ci-check.sh

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || echo "")"
if [ -z "$ROOT" ]; then echo "Error: not inside a git repository" >&2; exit 1; fi

echo "=== Format ==="
bun run --cwd "$ROOT" format

echo ""
echo "=== Validate ==="
bash "$ROOT/scripts/validate.sh"
