#!/usr/bin/env bash
# scripts/lib/worktree.sh — Worktree lifecycle for per-issue lane isolation
#
# Worktrees live at: ~/source/sera-wt/<slug>/
# Based on: origin/sera20
#
# Usage:
#   source "${SCRIPT_DIR}/lib/worktree.sh"
#   worktree_create "issue-142-memory-block" 142
#   worktree_bootstrap "issue-142-memory-block"
#   worktree_remove "issue-142-memory-block"

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# worktree.sh lives in scripts/lib/ — go up two levels to reach project root
: "${SCRIPT_DIR:=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"
SERA_ROOT="${SERA_ROOT:-$(git -C "$SCRIPT_DIR/../.." rev-parse --show-toplevel 2>/dev/null || echo "$SCRIPT_DIR/../..")}"
WORKTREE_PARENT="${WORKTREE_PARENT:-${HOME}/source/sera-wt}"
BASE_BRANCH="origin/sera20"
REPO="${SERA_ROOT}"

# ── Ensure worktree parent exists ─────────────────────────────────────────────
worktree_init() {
  mkdir -p "$WORKTREE_PARENT"
  echo "[worktree] Parent dir: ${WORKTREE_PARENT}"
}

# ── Create a new worktree ────────────────────────────────────────────────────

# worktree_create <slug> [issue_number]
# Creates: git worktree add <parent>/<slug> -b <slug> <base_branch>
# Runs bootstrap hook after creation.
worktree_create() {
  local slug="$1"
  local issue="${2:-}"

  # Validate slug to prevent path traversal
  if [[ ! "$slug" =~ ^[a-zA-Z0-9][-a-zA-Z0-9_]*$ ]]; then
    echo "[worktree] ERROR: slug must match ^[a-zA-Z0-9][-a-zA-Z0-9_]*$ — got: ${slug}" >&2
    return 1
  fi

  local wt_path="${WORKTREE_PARENT}/${slug}"

  worktree_init

  if [[ -d "$wt_path" ]]; then
    echo "[worktree] Already exists: ${wt_path}"
    return 0
  fi

  echo "[worktree] Creating worktree: ${slug} from ${BASE_BRANCH}"

  git -C "$REPO" worktree add "$wt_path" -b "$slug" "$BASE_BRANCH"

  # Record issue number in a file if provided
  if [[ -n "$issue" ]]; then
    echo "$issue" > "${wt_path}/.issue_number"
  fi

  echo "[worktree] Created: ${wt_path}"
  echo "[worktree] Branch: ${slug}"

  # Auto-bootstrap deps
  worktree_bootstrap "$slug"

  echo "[worktree] Ready: ${wt_path}"
}

# ── Bootstrap deps in a worktree ─────────────────────────────────────────────

# worktree_bootstrap <slug>
# Runs ops/worktree-bootstrap.sh if it exists.
worktree_bootstrap() {
  local slug="$1"
  local wt_path="${WORKTREE_PARENT}/${slug}"
  local bootstrap_script="${SERA_ROOT}/ops/worktree-bootstrap.sh"

  if [[ ! -d "$wt_path" ]]; then
    echo "[worktree] ERROR: worktree not found: ${wt_path}" >&2
    return 1
  fi

  echo "[worktree] Bootstrapping: ${slug}"

  # Symlink .env from parent if it exists and not already present
  local parent_env="${SERA_ROOT}/.env"
  local wt_env="${wt_path}/.env"
  if [[ -f "$parent_env" && ! -e "$wt_env" ]]; then
    ln -sfn "$parent_env" "$wt_env"
    echo "[worktree] Symlinked .env"
  fi

  # Run project-specific bootstrap if it exists
  if [[ -x "$bootstrap_script" ]]; then
    bash "$bootstrap_script" "$wt_path"
  else
    # Default: just fetch deps
    (cd "$wt_path" && cargo fetch 2>/dev/null || true)
    echo "[worktree] Fetched deps (or skipped — not an error)"
  fi

  echo "[worktree] Bootstrap complete: ${slug}"
}

# ── Remove a worktree ────────────────────────────────────────────────────────

# worktree_remove <slug>
# Only removes if worktree is in reclaimable state in ledger (or force flag).
# worktree_remove <slug> [force]
worktree_remove() {
  local slug="$1"
  local force="${2:-}"
  local wt_path="${WORKTREE_PARENT}/${slug}"

  if [[ ! -d "$wt_path" ]]; then
    echo "[worktree] Not found: ${wt_path}"
    return 0
  fi

  if [[ "$force" != "force" ]]; then
    echo "[worktree] REFUSING to remove ${slug} without explicit 'force' flag."
    echo "[worktree] Worktree path: ${wt_path}"
    echo "[worktree] Run 'git -C ${REPO} worktree remove ${wt_path}' manually if intended."
    return 1
  fi

  echo "[worktree] Removing worktree: ${slug}"
  git -C "$REPO" worktree remove "$wt_path" 2>/dev/null || \
    git -C "$REPO" worktree prune 2>/dev/null || true
  echo "[worktree] Removed: ${wt_path}"
}

# ── List all SERA worktrees ───────────────────────────────────────────────────
worktree_list() {
  echo "=== SERA Worktrees ==="
  git -C "$REPO" worktree list | grep -v "bare" || echo "none"
  echo ""
  echo "=== Worktree Parent ==="
  ls -la "$WORKTREE_PARENT" 2>/dev/null || echo "directory does not exist"
}

# ── Prune stale worktree references ─────────────────────────────────────────
worktree_prune() {
  echo "[worktree] Pruning stale worktree references..."
  git -C "$REPO" worktree prune
  echo "[worktree] Prune complete"
}

# ── Get worktree path for a slug ─────────────────────────────────────────────
worktree_path() {
  local slug="$1"
  # Validate on every use — defense in depth for path traversal prevention
  if [[ ! "$slug" =~ ^[a-zA-Z0-9][-a-zA-Z0-9_]*$ ]]; then
    echo "[worktree] ERROR: invalid slug: ${slug}" >&2
    return 1
  fi
  echo "${WORKTREE_PARENT}/${slug}"
}

worktree_exists() {
  local slug="$1"
  [[ "$slug" =~ ^[a-zA-Z0-9][-a-zA-Z0-9_]*$ ]] || return 1
  [[ -d "${WORKTREE_PARENT}/${slug}" ]]
}
