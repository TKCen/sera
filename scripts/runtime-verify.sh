#!/usr/bin/env bash
# Runtime verification: restart containers and check health.
# Usage: bash scripts/runtime-verify.sh
#
# Restarts core + web containers, waits for health, and verifies
# key endpoints return expected responses.

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

pass() { echo -e "${GREEN}  ✓ $1${NC}"; }
fail() { echo -e "${RED}  ✗ $1${NC}"; }

ERRORS=0

check() {
  local name="$1" url="$2" expect="${3:-200}"
  local code
  code=$(curl -s -o /dev/null -w "%{http_code}" "$url" 2>/dev/null || echo "000")
  if [ "$code" = "$expect" ]; then
    pass "$name (HTTP $code)"
  else
    fail "$name (HTTP $code, expected $expect)"
    ERRORS=$((ERRORS + 1))
  fi
}

check_json_field() {
  local name="$1" url="$2" field="$3" expect="$4"
  local val
  val=$(curl -s "$url" 2>/dev/null | python -c "import sys,json; print(json.load(sys.stdin)$field)" 2>/dev/null || echo "FAIL")
  if [ "$val" = "$expect" ]; then
    pass "$name ($field=$val)"
  else
    fail "$name ($field=$val, expected $expect)"
    ERRORS=$((ERRORS + 1))
  fi
}

echo "=== Runtime Verification ==="
echo ""

echo -e "${YELLOW}Restarting containers...${NC}"
docker restart sera-core sera-web > /dev/null 2>&1

echo -e "${YELLOW}Waiting for startup (10s)...${NC}"
sleep 10

echo ""
echo "Step 1: Health checks"
check_json_field "Core health" "http://localhost:3001/api/health/detail" "['status']" "healthy"
check "Web UI" "http://localhost:3000"

echo ""
echo "Step 2: API endpoints"
check "Agents list" "http://localhost:3001/api/agents"
check "Providers list" "http://localhost:3001/api/providers"
check "Templates list" "http://localhost:3001/api/templates"

echo ""
if [ "$ERRORS" -gt 0 ]; then
  echo -e "${RED}=== ${ERRORS} check(s) failed ===${NC}"
  exit 1
else
  echo -e "${GREEN}=== All runtime checks passed ===${NC}"
fi
