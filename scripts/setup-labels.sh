#!/usr/bin/env bash
# Creates GitHub labels for the multi-agent workflow.
# Run once: bash scripts/setup-labels.sh

set -euo pipefail

REPO="TKCen/sera"

create_label() {
  local name="$1" color="$2" description="$3"
  gh label create "$name" --repo "$REPO" --color "$color" --description "$description" --force
}

echo "Creating agent workflow labels..."

# Status
create_label "agent-ready"       "0E8A16" "Issue is fully specified and ready for an agent"
create_label "agent-work"        "1D76DB" "PR was created by an agent"
create_label "needs-human"       "D93F0B" "Agent is blocked, needs human input"
create_label "in-review"         "FBCA04" "PR is open and awaiting review"

# Assignment
create_label "assign-to-claude"      "7B61FF" "Best suited for Claude Code"
create_label "assign-to-jules"       "4285F4" "Best suited for Jules"
create_label "assign-to-gemini"      "34A853" "Best suited for Gemini CLI"
create_label "assign-to-antigravity" "EA4335" "Best suited for Antigravity"

# Complexity
create_label "complexity:trivial" "C5DEF5" "Single file, < 30 min"
create_label "complexity:small"   "BFD4F2" "1-2 files, well-defined"
create_label "complexity:medium"  "A2C4E0" "3-5 files, needs architecture docs"
create_label "complexity:large"   "7BA7CC" "5+ files, needs planning phase"

# Area
create_label "area:core"  "D4C5F9" "core/ workspace"
create_label "area:web"   "F9D4C5" "web/ workspace"
create_label "area:e2e"   "C5F9D4" "e2e/ tests"
create_label "area:infra" "F9F4C5" "Docker, CI, scripts"
create_label "area:docs"  "E8E8E8" "Documentation only"

echo "Done! Labels created on $REPO"
