#!/usr/bin/env bash
# scripts/lib/wrapperLedger.sh — JSON ledger for lane orchestration state
#
# Ledger file: ~/.sera/wrapper_ledger.json
# Schema per entry: slug, branch, cwd, status, created_at, last_lane,
#                    session_names[], pr, issue, taskId, artifact_root
#
# Status values: active | waiting_on_operator | review_ready | completed | stale | reclaimable
#
# Usage (source this file first):
#   source "${SCRIPT_DIR}/lib/wrapperLedger.sh"
#   export LEDGER_FILE="${HOME}/.sera/wrapper_ledger.json"
#   ledger_create "issue-142" "issue-142" "/path/to/wt" "TASK-001"
#   ledger_update "issue-142" "status=review_ready" "last_lane=omx"
#   ledger_get "issue-142"
#   ledger_list_active

set -euo pipefail

LEDGER_FILE="${LEDGER_FILE:-${HOME}/.sera/wrapper_ledger.json}"
LEDGER_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/ledger"

# ── Init ─────────────────────────────────────────────────────────────────────
ledger_ensure() {
  mkdir -p "$(dirname "$LEDGER_FILE")"
  if [[ ! -f "$LEDGER_FILE" ]]; then
    printf '[]\n' > "$LEDGER_FILE"
  fi
}

# ── Create ───────────────────────────────────────────────────────────────────
# ledger_create <slug> <branch> <cwd> <taskId> [issue]
ledger_create() {
  local slug="$1" branch="$2" cwd="$3" task_id="$4" issue="${5:-}"
  ledger_ensure
  local artifact_root="${cwd}/.omc/runtime/${slug}"
  mkdir -p "$artifact_root"

  # Each Python script runs in its own process — no set -e bleed
  python3 "${LEDGER_LIB_DIR}/ledger_create.py" \
    "$LEDGER_FILE" "$slug" "$branch" "$cwd" "$task_id" "$artifact_root" "$issue"
  echo "[ledger] Created entry: ${slug}"
}

# ── Update ───────────────────────────────────────────────────────────────────
# ledger_update <slug> "field=value" ...
ledger_update() {
  local slug="$1"; shift
  python3 "${LEDGER_LIB_DIR}/ledger_update.py" "$LEDGER_FILE" "$slug" "$@"
}

# ── Get ─────────────────────────────────────────────────────────────────────
# ledger_get <slug> [field]
ledger_get() {
  local slug="$1" field="${2:-}"
  python3 "${LEDGER_LIB_DIR}/ledger_get.py" "$LEDGER_FILE" "$slug" "$field"
}

# ── List ─────────────────────────────────────────────────────────────────────
# ledger_list [status]
ledger_list() {
  python3 "${LEDGER_LIB_DIR}/ledger_list.py" "$LEDGER_FILE" "${1:-}"
}
ledger_list_active() { ledger_list "active"; }

# ── Add session ──────────────────────────────────────────────────────────────
# ledger_add_session <slug> <session>
ledger_add_session() {
  python3 "${LEDGER_LIB_DIR}/ledger_add_session.py" "$LEDGER_FILE" "$1" "$2"
}

# ── Mark reclaimable ────────────────────────────────────────────────────────
# ledger_mark_reclaimable <slug> [--pr N]
ledger_mark_reclaimable() {
  local slug="$1"; shift
  ledger_update "$slug" "status=reclaimable"
  [[ "${1:-}" == "--pr" && -n "${2:-}" ]] && ledger_update "$slug" "pr=${2}"
  echo "[ledger] Marked ${slug} as reclaimable"
}

# ── Remove ──────────────────────────────────────────────────────────────────
# ledger_remove <slug>
ledger_remove() {
  python3 "${LEDGER_LIB_DIR}/ledger_remove.py" "$LEDGER_FILE" "$1"
  echo "[ledger] Removed ${1}"
}

# ── GC ──────────────────────────────────────────────────────────────────────
ledger_gc() {
  ledger_ensure
  printf '%s\n' "=== Reclaimable entries ==="
  ledger_list "reclaimable"
  printf '\n%s\n' "Run ledger_remove <slug> to delete entries."
}
