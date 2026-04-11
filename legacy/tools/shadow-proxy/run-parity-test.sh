#!/usr/bin/env bash
# run-parity-test.sh — Send a representative set of API requests through the
# shadow proxy so parity diffs are captured in the JSONL log.
#
# Usage:
#   bash tools/shadow-proxy/run-parity-test.sh [PROXY_URL] [API_KEY]
#
# Defaults:
#   PROXY_URL = http://localhost:3000
#   API_KEY   = sera_bootstrap_dev_123

set -euo pipefail

PROXY_URL="${1:-http://localhost:3000}"
API_KEY="${2:-sera_bootstrap_dev_123}"
AUTH="Authorization: Bearer ${API_KEY}"

PASS=0
FAIL=0
SKIP=0

# Colour helpers (no-op if not a terminal)
if [ -t 1 ]; then
  GREEN='\033[0;32m'
  RED='\033[0;31m'
  YELLOW='\033[1;33m'
  RESET='\033[0m'
else
  GREEN='' RED='' YELLOW='' RESET=''
fi

check() {
  local label="$1"
  local method="$2"
  local path="$3"
  local extra_flags="${4:-}"

  local status
  # shellcheck disable=SC2086
  status=$(curl -s -o /dev/null -w "%{http_code}" \
    -X "${method}" \
    -H "${AUTH}" \
    ${extra_flags} \
    "${PROXY_URL}${path}" 2>/dev/null) || status="000"

  if [ "$status" = "000" ]; then
    echo -e "  ${YELLOW}SKIP${RESET}  ${method} ${path}  (proxy unreachable)"
    SKIP=$((SKIP + 1))
  elif [ "$status" -ge 200 ] && [ "$status" -lt 500 ]; then
    echo -e "  ${GREEN}PASS${RESET}  ${method} ${path}  [${status}]"
    PASS=$((PASS + 1))
  else
    echo -e "  ${RED}FAIL${RESET}  ${method} ${path}  [${status}]"
    FAIL=$((FAIL + 1))
  fi
}

echo "=== Shadow Proxy Parity Test ==="
echo "Proxy:  ${PROXY_URL}"
echo "Auth:   Bearer ${API_KEY:0:8}..."
echo ""

# ── Health endpoints (no auth required) ──────────────────────────────────────
echo "Health"
check "health"          GET /api/health
check "health/detailed" GET /api/health/detailed

# ── Core resource endpoints ───────────────────────────────────────────────────
echo ""
echo "Agents"
check "agents list"     GET /api/agents
check "agents search"   GET "/api/agents?limit=10"

echo ""
echo "Templates"
check "templates list"  GET /api/templates

echo ""
echo "Providers"
check "providers list"  GET /api/providers

echo ""
echo "Tools"
check "tools list"      GET /api/tools

echo ""
echo "Schedules"
check "schedules list"  GET /api/schedules

echo ""
echo "Circles"
check "circles list"    GET /api/circles

echo ""
echo "Audit"
check "audit log"       GET /api/audit

echo ""
echo "Models"
check "models list"     GET /api/models

echo ""
echo "Sessions"
check "sessions list"   GET /api/sessions

echo ""
# ── Summary ───────────────────────────────────────────────────────────────────
TOTAL=$((PASS + FAIL + SKIP))
echo "=== Results ==="
echo -e "  ${GREEN}Passed${RESET}: ${PASS}/${TOTAL}"
if [ "${FAIL}" -gt 0 ]; then
  echo -e "  ${RED}Failed${RESET}: ${FAIL}/${TOTAL}"
fi
if [ "${SKIP}" -gt 0 ]; then
  echo -e "  ${YELLOW}Skipped${RESET}: ${SKIP}/${TOTAL}  (proxy not reachable)"
fi
echo ""
echo "Parity diffs are captured in the shadow proxy's DIFF_LOG_PATH."
echo "Run the report tool to analyse them:"
echo "  docker exec sera-shadow-proxy node --experimental-strip-types report.ts"
