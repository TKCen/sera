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

# Dev bootstrap API key — matches SERA_BOOTSTRAP_API_KEY in docker-compose.dev.yaml
API_KEY="${SERA_BOOTSTRAP_API_KEY:-sera_bootstrap_dev_123}"
AUTH_HEADER="Authorization: Bearer ${API_KEY}"

pass() { echo -e "${GREEN}  ✓ $1${NC}"; }
fail() { echo -e "${RED}  ✗ $1${NC}"; }
warn() { echo -e "${YELLOW}  ⚠ $1${NC}"; }

ERRORS=0
WARNINGS=0

check() {
  local name="$1" url="$2" expect="${3:-200}" auth="${4:-yes}"
  local code
  if [ "$auth" = "yes" ]; then
    code=$(curl -s -o /dev/null -w "%{http_code}" -H "$AUTH_HEADER" "$url" 2>/dev/null || echo "000")
  else
    code=$(curl -s -o /dev/null -w "%{http_code}" "$url" 2>/dev/null || echo "000")
  fi
  if [ "$code" = "$expect" ]; then
    pass "$name (HTTP $code)"
  else
    fail "$name (HTTP $code, expected $expect)"
    ERRORS=$((ERRORS + 1))
  fi
}

check_json_field() {
  local name="$1" url="$2" jq_filter="$3" expect="$4"
  local val
  # Use node for JSON parsing (always available, unlike python/jq)
  val=$(curl -s "$url" 2>/dev/null | node -e "
    let d=''; process.stdin.on('data',c=>d+=c); process.stdin.on('end',()=>{
      try { const o=JSON.parse(d); console.log(eval('o'+'.${jq_filter}')); }
      catch(e) { console.log('PARSE_ERROR'); }
    });
  " 2>/dev/null || echo "FAIL")
  if [ "$val" = "$expect" ]; then
    pass "$name (${jq_filter}=${val})"
  else
    fail "$name (${jq_filter}=${val}, expected $expect)"
    ERRORS=$((ERRORS + 1))
  fi
}

echo "=== Runtime Verification ==="
echo ""

echo -e "${YELLOW}Restarting containers...${NC}"
docker restart sera-core sera-web > /dev/null 2>&1

echo -e "${YELLOW}Waiting for startup (12s)...${NC}"
sleep 12

echo ""
echo "Step 1: Health checks"
# Health endpoint is public (no auth required)
# Note: overall status may be "unhealthy" if squid is down (#363),
# so we check the endpoint is reachable rather than the aggregate status.
check "Core health endpoint" "http://localhost:3001/api/health/detail" "200" "no"
check "Web UI reachable" "http://localhost:3000" "200" "no"

# Check for known unhealthy components
echo ""
echo "Step 2: Component health"
HEALTH_JSON=$(curl -s http://localhost:3001/api/health/detail 2>/dev/null || echo '{}')
for component in database centrifugo docker qdrant pg-boss; do
  status=$(echo "$HEALTH_JSON" | node -e "
    let d=''; process.stdin.on('data',c=>d+=c); process.stdin.on('end',()=>{
      try {
        const o=JSON.parse(d);
        const c=o.components?.find(x=>x.name==='${component}');
        console.log(c?.status||'missing');
      } catch(e) { console.log('error'); }
    });
  " 2>/dev/null || echo "error")
  if [ "$status" = "healthy" ]; then
    pass "$component"
  else
    fail "$component ($status)"
    ERRORS=$((ERRORS + 1))
  fi
done

# Squid is known-flaky on restart (#363) — warn instead of fail
squid_status=$(echo "$HEALTH_JSON" | node -e "
  let d=''; process.stdin.on('data',c=>d+=c); process.stdin.on('end',()=>{
    try {
      const o=JSON.parse(d);
      const c=o.components?.find(x=>x.name==='squid');
      console.log(c?.status||'missing');
    } catch(e) { console.log('error'); }
  });
" 2>/dev/null || echo "error")
if [ "$squid_status" = "healthy" ]; then
  pass "squid"
else
  warn "squid ($squid_status) — known restart issue (#363)"
  WARNINGS=$((WARNINGS + 1))
fi

echo ""
echo "Step 3: API endpoints"
# Note: /api/providers/list (not /api/providers) due to Express 5 sub-router matching
check "Agents list" "http://localhost:3001/api/agents"
check "Providers list" "http://localhost:3001/api/providers/list"
check "Templates list" "http://localhost:3001/api/templates"
check "Tools list" "http://localhost:3001/api/tools"
check "Sessions list" "http://localhost:3001/api/sessions"

echo ""
echo "Step 4: Container status"
for container in sera-core sera-web sera-db sera-qdrant sera-centrifugo; do
  status=$(docker inspect "$container" --format "{{.State.Health.Status}}" 2>/dev/null || echo "no-healthcheck")
  running=$(docker inspect "$container" --format "{{.State.Running}}" 2>/dev/null || echo "false")
  if [ "$running" != "true" ]; then
    fail "$container (not running)"
    ERRORS=$((ERRORS + 1))
  elif [ "$status" = "healthy" ] || [ "$status" = "no-healthcheck" ]; then
    pass "$container ($status)"
  elif [ "$container" = "sera-web" ]; then
    # sera-web healthcheck is known-broken (#364) — wget can't reach localhost
    warn "$container ($status) — known healthcheck issue (#364)"
    WARNINGS=$((WARNINGS + 1))
  else
    fail "$container ($status)"
    ERRORS=$((ERRORS + 1))
  fi
done

echo ""
if [ "$ERRORS" -gt 0 ]; then
  echo -e "${RED}=== ${ERRORS} check(s) failed, ${WARNINGS} warning(s) ===${NC}"
  exit 1
elif [ "$WARNINGS" -gt 0 ]; then
  echo -e "${YELLOW}=== All checks passed with ${WARNINGS} known warning(s) ===${NC}"
else
  echo -e "${GREEN}=== All runtime checks passed ===${NC}"
fi
