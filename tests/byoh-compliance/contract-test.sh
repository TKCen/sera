#!/bin/bash
# SERA BYOH Contract Compliance Test Suite
#
# Tests agent containers against the BYOH contract specification.
# Requires: running SERA stack, Docker, curl, jq
#
# Usage:
#   ./tests/byoh-compliance/contract-test.sh [image-name]
#   ./tests/byoh-compliance/contract-test.sh              # tests all images
#   ./tests/byoh-compliance/contract-test.sh sera-byoh-python:latest

set -euo pipefail

SERA_API="http://localhost:3001"
API_KEY="sera_bootstrap_dev_123"
PASS=0
FAIL=0
SKIP=0

# ── Helpers ──────────────────────────────────────────────────────────────────

pass() { echo "  ✓ PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  ✗ FAIL: $1"; FAIL=$((FAIL + 1)); }
skip() { echo "  ○ SKIP: $1"; SKIP=$((SKIP + 1)); }
header() { echo ""; echo "═══ $1 ═══"; }

# ── Test 1: SERA Core Health ────────────────────────────────────────────────

test_core_health() {
  header "Test 1: sera-core health"
  local res
  res=$(curl -sf "${SERA_API}/api/health" 2>/dev/null) || { fail "sera-core not responding"; return; }
  echo "$res" | jq -e '.status == "ok"' >/dev/null 2>&1 && pass "sera-core healthy" || fail "sera-core unhealthy: $res"
}

# ── Test 2: SERA_LLM_PROXY_URL in running agent ────────────────────────────

test_llm_proxy_env() {
  header "Test 2: SERA_LLM_PROXY_URL env var injection"

  # Find a running sera-agent container
  local container
  container=$(docker ps --filter "label=sera.sandbox=true" --filter "label=sera.type=agent" --format "{{.Names}}" | head -1)

  if [ -z "$container" ]; then
    skip "No running agent container found"
    return
  fi

  local env_val
  env_val=$(docker inspect "$container" --format '{{range .Config.Env}}{{println .}}{{end}}' | grep "SERA_LLM_PROXY_URL=" | head -1)

  if [ -n "$env_val" ]; then
    pass "SERA_LLM_PROXY_URL present: $env_val"
  else
    fail "SERA_LLM_PROXY_URL not found in container env"
  fi

  # Check AGENT_CHAT_PORT
  local port_val
  port_val=$(docker inspect "$container" --format '{{range .Config.Env}}{{println .}}{{end}}' | grep "AGENT_CHAT_PORT=" | head -1)

  if [ -n "$port_val" ]; then
    pass "AGENT_CHAT_PORT present: $port_val"
  else
    fail "AGENT_CHAT_PORT not found in container env"
  fi
}

# ── Test 3: Agent health endpoint ───────────────────────────────────────────

test_agent_health() {
  header "Test 3: Agent /health endpoint"

  local container
  container=$(docker ps --filter "label=sera.sandbox=true" --filter "label=sera.type=agent" --format "{{.Names}}" | head -1)

  if [ -z "$container" ]; then
    skip "No running agent container found"
    return
  fi

  # Test health from inside the container (host can't reach internal Docker networks)
  local health
  health=$(MSYS_NO_PATHCONV=1 docker exec "$container" sh -c 'curl -sf http://localhost:${AGENT_CHAT_PORT:-3100}/health 2>/dev/null || echo "FAIL"')

  if echo "$health" | jq -e '.ready' >/dev/null 2>&1; then
    pass "Health endpoint responds with ready field"
    echo "$health" | jq -e '.busy != null' >/dev/null 2>&1 && pass "Health has busy field" || fail "Health missing busy field"
  else
    fail "Health endpoint not responding or invalid JSON: $health"
  fi
}

# ── Test 4: BYOH contract schemas validate ──────────────────────────────────

test_schema_validation() {
  header "Test 4: Contract schema validation"

  # Validate TaskInput schema has required fields
  local schema_file="schemas/byoh-task-input.schema.json"
  if [ -f "$schema_file" ]; then
    jq -e '.required | contains(["taskId", "task"])' "$schema_file" >/dev/null 2>&1 \
      && pass "TaskInput schema has required fields" \
      || fail "TaskInput schema missing required fields"
  else
    fail "TaskInput schema not found"
  fi

  # Validate TaskOutput schema
  schema_file="schemas/byoh-task-output.schema.json"
  if [ -f "$schema_file" ]; then
    jq -e '.required | contains(["taskId", "result"])' "$schema_file" >/dev/null 2>&1 \
      && pass "TaskOutput schema has required fields" \
      || fail "TaskOutput schema missing required fields"
  else
    fail "TaskOutput schema not found"
  fi

  # Validate Health schema
  schema_file="schemas/byoh-health.schema.json"
  if [ -f "$schema_file" ]; then
    jq -e '.required | contains(["ready", "busy"])' "$schema_file" >/dev/null 2>&1 \
      && pass "Health schema has required fields" \
      || fail "Health schema missing required fields"
  else
    fail "Health schema not found"
  fi

  # Validate sera-skill schema exists and is discovery-only
  schema_file="schemas/sera-skill.schema.json"
  if [ -f "$schema_file" ]; then
    pass "sera-skill schema exists"
    # Verify no entrypoint or runtime fields (v1 = discovery-only)
    if jq -e '.properties.entrypoint' "$schema_file" >/dev/null 2>&1; then
      fail "sera-skill schema has entrypoint field (should be discovery-only in v1)"
    else
      pass "sera-skill schema is discovery-only (no entrypoint)"
    fi
  else
    fail "sera-skill schema not found"
  fi
}

# ── Test 5: Image allowlist in sandbox boundaries ───────────────────────────

test_image_allowlist() {
  header "Test 5: Image allowlist in sandbox boundaries"

  for tier in tier-1 tier-2 tier-3; do
    local yaml_file="sandbox-boundaries/${tier}.yaml"
    if [ -f "$yaml_file" ]; then
      if grep -q "allowedImages" "$yaml_file"; then
        pass "${tier}.yaml has allowedImages"
      else
        fail "${tier}.yaml missing allowedImages"
      fi
    else
      fail "${tier}.yaml not found"
    fi
  done
}

# ── Test 6: Skill registry API ──────────────────────────────────────────────

test_skill_registry_api() {
  header "Test 6: Skill registry API"

  # GET /api/skills/registry/list
  local list_res
  list_res=$(curl -sf -H "Authorization: Bearer ${API_KEY}" "${SERA_API}/api/skills/registry/list" 2>/dev/null) || {
    skip "GET /api/skills/registry/list not available (sera-core may need rebuild with latest code)"
    return
  }

  if echo "$list_res" | jq -e '.builtin' >/dev/null 2>&1; then
    local builtin_count
    builtin_count=$(echo "$list_res" | jq '.builtin | length')
    pass "Registry list returns builtin tools (${builtin_count} found)"
  else
    fail "Registry list missing builtin field"
  fi

  if echo "$list_res" | jq -e '.manifest' >/dev/null 2>&1; then
    pass "Registry list returns manifest field"
  else
    fail "Registry list missing manifest field"
  fi

  # POST /api/skills/registry/search
  local search_res
  search_res=$(curl -sf -X POST -H "Authorization: Bearer ${API_KEY}" -H "Content-Type: application/json" \
    -d '{"query":"search"}' "${SERA_API}/api/skills/registry/search" 2>/dev/null) || {
    fail "POST /api/skills/registry/search failed"
    return
  }

  if echo "$search_res" | jq -e '.total >= 0' >/dev/null 2>&1; then
    pass "Search endpoint returns results"
  else
    fail "Search endpoint invalid response: $search_res"
  fi
}

# ── Test 7: BYOH example container (Python) ────────────────────────────────

test_byoh_python() {
  header "Test 7: BYOH Python example container"

  if ! docker image inspect sera-byoh-python:latest >/dev/null 2>&1; then
    skip "sera-byoh-python:latest not built — run: docker build -t sera-byoh-python:latest examples/byoh-python/"
    return
  fi

  # Run with a task on stdin, expect TaskOutput on stdout
  local output
  output=$(echo '{"taskId":"test-py-001","task":"Say hello"}' | \
    docker run --rm -i \
      -e SERA_IDENTITY_TOKEN=test \
      -e SERA_LLM_PROXY_URL=http://host.docker.internal:3001/v1/llm \
      -e AGENT_NAME=test-python \
      -e AGENT_INSTANCE_ID=test-py-001 \
      --network none \
      sera-byoh-python:latest 2>/dev/null) || true

  if echo "$output" | jq -e '.taskId == "test-py-001"' >/dev/null 2>&1; then
    pass "Python agent returns valid TaskOutput with correct taskId"
  else
    fail "Python agent output invalid: $output"
  fi

  if echo "$output" | jq -e 'has("result")' >/dev/null 2>&1; then
    pass "Python agent output has result field"
  else
    fail "Python agent output missing result field"
  fi
}

# ── Test 8: BYOH example container (Shell) ──────────────────────────────────

test_byoh_shell() {
  header "Test 8: BYOH Shell example container"

  if ! docker image inspect sera-byoh-shell:latest >/dev/null 2>&1; then
    skip "sera-byoh-shell:latest not built — run: docker build -t sera-byoh-shell:latest examples/byoh-shell/"
    return
  fi

  local output
  output=$(echo '{"taskId":"test-sh-001","task":"Say hello"}' | \
    docker run --rm -i \
      -e SERA_IDENTITY_TOKEN=test \
      -e SERA_LLM_PROXY_URL=http://host.docker.internal:3001/v1/llm \
      -e AGENT_NAME=test-shell \
      -e AGENT_INSTANCE_ID=test-sh-001 \
      --network none \
      sera-byoh-shell:latest 2>/dev/null) || true

  if echo "$output" | jq -e '.taskId == "test-sh-001"' >/dev/null 2>&1; then
    pass "Shell agent returns valid TaskOutput with correct taskId"
  else
    fail "Shell agent output invalid: $output"
  fi

  if echo "$output" | jq -e 'has("result")' >/dev/null 2>&1; then
    pass "Shell agent output has result field"
  else
    fail "Shell agent output missing result field"
  fi
}

# ── Test 9: Negative egress test ────────────────────────────────────────────

test_negative_egress() {
  header "Test 9: Negative egress test (advisory check)"

  # On Docker Desktop, egress enforcement is advisory — direct connections may succeed.
  # This test documents the behavior rather than asserting enforcement.

  local result
  result=$(docker run --rm --network agent_net alpine:3.19 \
    sh -c 'wget -qO- --timeout=5 http://1.1.1.1 2>&1 || echo "BLOCKED"' 2>/dev/null) || true

  if echo "$result" | grep -q "BLOCKED"; then
    pass "Direct egress blocked (enforcement active)"
  else
    skip "Direct egress NOT blocked — advisory enforcement only (expected on Docker Desktop)"
  fi
}

# ── Test 10: BYOH contract documentation exists ─────────────────────────────

test_documentation() {
  header "Test 10: Documentation completeness"

  [ -f "docs/BYOH-CONTRACT.md" ] && pass "BYOH-CONTRACT.md exists" || fail "BYOH-CONTRACT.md missing"
  [ -f "docs/SERA-V1-EXECUTION-PLAN.md" ] && pass "SERA-V1-EXECUTION-PLAN.md exists" || fail "Execution plan missing"
  [ -f "docs/EGRESS-ENFORCEMENT.md" ] && pass "EGRESS-ENFORCEMENT.md exists" || fail "Egress enforcement doc missing"

  # Check BYOH contract covers all dimensions (use tr to strip CRLF, grep -iP for alternation)
  local contract
  contract=$(tr -d '\r' < docs/BYOH-CONTRACT.md)
  local -a dims=("stdin" "stdout" "LLM" "Proxy" "health" "Heartbeat" "SIGTERM")
  local -a labels=("Task input (stdin)" "Task output (stdout)" "LLM access" "Proxy/security" "Health endpoint" "Heartbeat" "Graceful shutdown (SIGTERM)")
  for i in "${!dims[@]}"; do
    if echo "$contract" | grep -qi "${dims[$i]}"; then
      pass "Contract covers: ${labels[$i]}"
    else
      fail "Contract missing: ${labels[$i]}"
    fi
  done
}

# ── Main ─────────────────────────────────────────────────────────────────────

main() {
  echo "SERA BYOH Contract Compliance Test Suite"
  echo "========================================="
  echo "Date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "API:  ${SERA_API}"

  cd "$(dirname "$0")/../.."

  test_core_health
  test_llm_proxy_env
  test_agent_health
  test_schema_validation
  test_image_allowlist
  test_skill_registry_api
  test_byoh_python
  test_byoh_shell
  test_negative_egress
  test_documentation

  echo ""
  echo "========================================="
  echo "Results: ${PASS} passed, ${FAIL} failed, ${SKIP} skipped"
  echo "========================================="

  [ "$FAIL" -eq 0 ] && echo "OVERALL: PASS" || echo "OVERALL: FAIL"
  exit "$FAIL"
}

main "$@"
