#!/usr/bin/env bash
# scripts/lib/common.sh — Shared utilities for sera-dev scripts

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# Output helpers
pass()   { echo -e "${GREEN}  ✓ $1${NC}"; }
fail()   { echo -e "${RED}  ✗ $1${NC}"; }
warn()   { echo -e "${YELLOW}  ⚠ $1${NC}"; }
info()   { echo -e "  $1"; }
header() { echo -e "\n${BOLD}${CYAN}=== $1 ===${NC}"; }

# Check if a command exists on PATH
# Usage: check_command cargo || warn "cargo not found"
check_command() {
  command -v "$1" >/dev/null 2>&1
}

# Return the git repo root, or empty string if not in a repo
find_repo_root() {
  git rev-parse --show-toplevel 2>/dev/null || echo ""
}
