#!/usr/bin/env bash
# scripts/lib/beads.sh — Bead utility functions for sera scripts
#
# Source this file to use: source "$(dirname "${BASH_SOURCE[0]}")/lib/beads.sh"
#
# Functions:
#   bead_claim <id>              — claim a bead (non-fatal)
#   bead_close <id>              — close a bead (non-fatal)
#   bead_show_title <id>         — print bead title to stdout
#   bead_gen_handoff <id> <file> — generate a handoff markdown file
#   bead_check_blockers <id>     — warn and return 1 if blocked

# Check if bd is available
_bd_available() {
  command -v bd >/dev/null 2>&1
}

# Claim a bead. Returns 0 on success or if bd unavailable.
bead_claim() {
  local id="$1"
  if ! _bd_available; then return 0; fi
  bd update "$id" --claim 2>/dev/null
}

# Close a bead. Returns 0 on success or if bd unavailable.
bead_close() {
  local id="$1"
  if ! _bd_available; then return 0; fi
  bd close "$id" 2>/dev/null
}

# Print bead title (single line). Prints empty string on failure.
bead_show_title() {
  local id="$1"
  if ! _bd_available; then echo ""; return 0; fi
  bd show "$id" --json 2>/dev/null \
    | grep '"title"' \
    | head -1 \
    | sed 's/.*"title": *"\(.*\)".*/\1/'
}

# Generate a handoff markdown file from bead content.
# Usage: bead_gen_handoff <id> <dest_file>
# Returns 0 on success, 1 on failure.
bead_gen_handoff() {
  local id="$1"
  local dest="$2"

  if ! _bd_available; then
    echo "[beads] bd not available — skipping handoff generation" >&2
    return 1
  fi

  local bead_json
  bead_json="$(bd show "$id" --json 2>/dev/null)" || { echo "[beads] Failed to fetch bead ${id}" >&2; return 1; }

  local title status priority issue_type assignee
  title="$(echo "$bead_json" | grep '"title"' | head -1 | sed 's/.*"title": *"\(.*\)".*/\1/')"
  status="$(echo "$bead_json" | grep '"status"' | head -1 | sed 's/.*"status": *"\(.*\)".*/\1/')"
  priority="$(echo "$bead_json" | grep '"priority"' | head -1 | sed 's/.*"priority": *\([0-9]*\).*/P\1/')"
  issue_type="$(echo "$bead_json" | grep '"issue_type"' | head -1 | sed 's/.*"issue_type": *"\(.*\)".*/\1/')"
  assignee="$(echo "$bead_json" | grep '"assignee"' | head -1 | sed 's/.*"assignee": *"\(.*\)".*/\1/')"

  local dest_dir
  dest_dir="$(dirname "$dest")"
  mkdir -p "$dest_dir"

  cat > "$dest" <<EOF
# Handoff — omc-sera-${id}

## Bead
${id} · ${title}  [${priority} · ${status}]

Type: ${issue_type}
Assignee: ${assignee}

## Context
Auto-generated from bead on $(date -u +"%Y-%m-%d %H:%M UTC")

## Bead Details

\`\`\`
$(bd show "$id" 2>/dev/null || echo "(unavailable)")
\`\`\`

## Mode
Use \`/ralph\` — persistent verify/fix loop.

## Stop Condition
All acceptance criteria met and bead closed with \`sera-dev finish ${id}\`.
EOF

  echo "[beads] Handoff written to: ${dest}"
}

# Check if a bead has unresolved blockers.
# Returns 0 (no blockers), 1 (has blockers).
bead_check_blockers() {
  local id="$1"
  if ! _bd_available; then return 0; fi

  local output
  output="$(bd show "$id" 2>/dev/null || true)"

  if echo "$output" | grep -qi "blocked"; then
    echo "[beads] Warning: bead ${id} may have unresolved blockers" >&2
    return 1
  fi
  return 0
}
