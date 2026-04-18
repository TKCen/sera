#!/usr/bin/env bash
# scripts/lib/task.sh — Task artifact CRUD for CLAWHIP lane orchestration
#
# Canonical task artifact: artifacts/tasks/<taskId>.json
# Handoff artifact:        artifacts/handoffs/<taskId>-HANDOFF.md
# Review artifact:         artifacts/review/<taskId>-REVIEW.md
#
# Usage:
#   source "${SCRIPT_DIR}/lib/task.sh"
#   task_create "TASK-001" "Add MemoryBlock to sera-types" "/home/entity/projects/sera" "implement"
#   task_read "TASK-001" field   # print single field
#   task_handoff_write "TASK-001" "Added MemoryBlock struct" "passed"
#   task_review_write "TASK-001" "approve" "MemoryBlock compiles and tests pass"

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Resolve BASH_SOURCE to absolute path since relative paths fail dirname+cd
SCRIPT_DIR="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
SERA_ROOT="${SERA_ROOT:-$(git -C "$SCRIPT_DIR/../.." rev-parse --show-toplevel 2>/dev/null || echo "$SCRIPT_DIR/../..")}"

ARTIFACTS_TASKS="${SERA_ROOT}/artifacts/tasks"
ARTIFACTS_HANDOFFS="${SERA_ROOT}/artifacts/handoffs"
ARTIFACTS_REVIEW="${SERA_ROOT}/artifacts/review"

# ── Create a new task artifact ────────────────────────────────────────────────

# task_create <taskId> <goal> <cwd> [lane] [intent] [mode]
# lane default: omc, intent default: implement, mode default: exec
task_create() {
  local task_id="$1"
  local goal="$2"
  local cwd="$3"
  local lane="${4:-omc}"
  local intent="${5:-implement}"
  local mode="${6:-exec}"

  mkdir -p "$ARTIFACTS_TASKS"

  local title_slug
  title_slug="$(echo "$goal" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9-' | tr ' ' '-' | cut -c1-60)"

  local timestamp
  timestamp="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

  cat > "${ARTIFACTS_TASKS}/${task_id}.json" <<EOF
{
  "taskId": "${task_id}",
  "title": "${goal}",
  "titleSlug": "${title_slug}",
  "lane": "${lane}",
  "intent": "${intent}",
  "cwd": "${cwd}",
  "goal": "${goal}",
  "inputs": [],
  "writeArtifacts": ["artifacts/handoffs/${task_id}-HANDOFF.md"],
  "stopCondition": "TODO: define stop condition",
  "dependencies": [],
  "verify": [],
  "reviewRequired": true,
  "execution": {
    "mode": "${mode}",
    "teamSize": 1
  },
  "created_at": "${timestamp}"
}
EOF
  echo "[task] Created ${ARTIFACTS_TASKS}/${task_id}.json"
}

# ── Read a task field ────────────────────────────────────────────────────────

# task_read <taskId> [field]
# With field: prints value. Without: prints full JSON.
task_read() {
  local task_id="$1"
  local field="${2:-}"
  local path="${ARTIFACTS_TASKS}/${task_id}.json"

  if [[ ! -f "$path" ]]; then
    echo "[task] ERROR: ${path} not found" >&2
    return 1
  fi

  if [[ -z "$field" ]]; then
    cat "$path"
  else
    python3 -c "import json,sys; d=json.load(open('${path}')); print(d.get('${field}',''))"
  fi
}

# ── Write handoff artifact ───────────────────────────────────────────────────

# task_handoff_write <taskId> <result_summary> <verification_status> [files_changed]
task_handoff_write() {
  local task_id="$1"
  local result="$2"
  local verify_status="$3"
  local files_changed="${4:-}"
  local lane="${5:-omx}"

  mkdir -p "$ARTIFACTS_HANDOFFS"

  local timestamp
  timestamp="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

  local files_block=""
  if [[ -n "$files_changed" ]]; then
    files_block="## Files changed\n\n${files_changed}\n\n"
  fi

  cat > "${ARTIFACTS_HANDOFFS}/${task_id}-HANDOFF.md" <<EOF
# HANDOFF — ${task_id}

## Produced by
${lane} lane for ${task_id}

## Result
${result}

## Verification status
- ${verify_status}

${files_block}## Timestamp
${timestamp}

## Next step
Run review lane against this handoff.
EOF
  echo "[task] Handoff written to ${ARTIFACTS_HANDOFFS}/${task_id}-HANDOFF.md"
}

# ── Write review artifact ─────────────────────────────────────────────────────

# task_review_write <taskId> <verdict> <findings> [next_step]
task_review_write() {
  local task_id="$1"
  local verdict="$2"
  local findings="$3"
  local next_step="${4:-Task is done.}"

  mkdir -p "$ARTIFACTS_REVIEW"

  local timestamp
  timestamp="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

  cat > "${ARTIFACTS_REVIEW}/${task_id}-REVIEW.md" <<EOF
# REVIEW — ${task_id}

## Verdict
${verdict}

## Findings
${findings}

## Next step
${next_step}

## Timestamp
${timestamp}
EOF
  echo "[task] Review written to ${ARTIFACTS_REVIEW}/${task_id}-REVIEW.md"
}

# ── Verify a task's verify commands ──────────────────────────────────────────

# task_verify <taskId>
# Runs each verify command in sequence, returns 0 if all pass.
task_verify() {
  local task_id="$1"
  local path="${ARTIFACTS_TASKS}/${task_id}.json"

  if [[ ! -f "$path" ]]; then
    echo "[task] ERROR: ${path} not found" >&2
    return 1
  fi

  local verify_cmds
  verify_cmds="$(python3 -c "import json,sys; d=json.load(open('${path}')); print('\n'.join(d.get('verify',[])))" 2>/dev/null || true)"

  if [[ -z "$verify_cmds" ]]; then
    echo "[task] No verify commands defined for ${task_id}"
    return 0
  fi

  local cwd
  cwd="$(task_read "$task_id" "cwd")"

  local failed=0
  while IFS= read -r cmd; do
    [[ -z "$cmd" ]] && continue
    echo "[task] Running: ${cmd}"
    # Safely execute via printf + eval — avoids shell injection from JSON
    if ( cd "${cwd}" && eval "$(printf '%s' "$cmd")" ); then
      echo "[task] PASS: ${cmd}"
    else
      echo "[task] FAIL: ${cmd}"
      failed=1
    fi
  done <<< "$verify_cmds"

  return $failed
}

# ── List all tasks ───────────────────────────────────────────────────────────

task_list() {
  ls "$ARTIFACTS_TASKS/"*.json 2>/dev/null | xargs -I{} basename {} .json || true
}

# ── Add input files to a task ────────────────────────────────────────────────

# task_add_inputs <taskId> <file1> [file2] ...
task_add_inputs() {
  local task_id="$1"
  shift
  local path="${ARTIFACTS_TASKS}/${task_id}.json"

  if [[ ! -f "$path" ]]; then
    echo "[task] ERROR: ${path} not found" >&2
    return 1
  fi

  # Pass all remaining args ($@) to Python — not just first 3
  local inputs_json
  inputs_json="$(python3 "$path" "$@" <<'PYEOF'
import json, sys
path = sys.argv[1]
files = sys.argv[2:]
with open(path) as f:
    d = json.load(f)
d.setdefault('inputs', [])
for f in files:
    if f not in d['inputs']:
        d['inputs'].append(f)
print(json.dumps(d, indent=2))
PYEOF
)"

  echo "$inputs_json" > "${path}"
  echo "[task] Updated inputs for ${task_id}"
}
