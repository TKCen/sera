#!/usr/bin/env bash
# List open issues sorted by priority labels.
# Usage: bash scripts/issue-triage.sh [--limit N]

set -euo pipefail

LIMIT="${1:-20}"
if [ "$1" = "--limit" ] 2>/dev/null; then LIMIT="${2:-20}"; fi

echo "=== Open Issues (top $LIMIT) ==="
echo ""

# High priority first, then medium, then rest
for label in "priority:high" "priority:medium" ""; do
  if [ -n "$label" ]; then
    issues=$(gh issue list --state open --label "$label" --limit "$LIMIT" \
      --json number,title,labels \
      --jq '.[] | "\(.number)\t\(.title)\t\(.labels | map(.name) | join(", "))"' 2>/dev/null)
    [ -z "$issues" ] && continue
    echo "  [$label]"
  else
    issues=$(gh issue list --state open --limit "$LIMIT" \
      --json number,title,labels \
      --jq '.[] | select((.labels | map(.name) | any(startswith("priority:"))) | not) | "\(.number)\t\(.title)\t\(.labels | map(.name) | join(", "))"' 2>/dev/null)
    [ -z "$issues" ] && continue
    echo "  [no priority]"
  fi

  while IFS=$'\t' read -r num title labels; do
    printf "    #%-4s  %-60s  %s\n" "$num" "$title" "$labels"
  done <<< "$issues"
  echo ""
done
