#!/usr/bin/env bash
# List open PRs with their CI status and mergeability.
# Usage: bash scripts/pr-triage.sh

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

prs=$(gh pr list --state open --json number,title,author,headRefName \
  --jq '.[] | "\(.number)\t\(.author.login)\t\(.title)"')

if [ -z "$prs" ]; then
  echo "No open PRs."
  exit 0
fi

echo "=== Open PRs ==="
echo ""

while IFS=$'\t' read -r num author title; do
  ci=$(gh pr checks "$num" --json name,state \
    --jq '[.[] | select(.name=="validate")] | .[0].state // "none"' 2>/dev/null || echo "unknown")

  case "$ci" in
    SUCCESS) status="${GREEN}✓ pass${NC}" ;;
    FAILURE) status="${RED}✗ fail${NC}" ;;
    *)       status="${YELLOW}? ${ci}${NC}" ;;
  esac

  printf "  #%-4s %-12s [%b]  %s\n" "$num" "$author" "$status" "$title"
done <<< "$prs"
