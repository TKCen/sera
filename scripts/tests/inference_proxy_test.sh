#!/usr/bin/env bash
# scripts/tests/inference_proxy_test.sh — Unit tests for sera-kg3g launcher
# enforcement library.
#
# Pure bash, no external deps. Runs as: bash scripts/tests/inference_proxy_test.sh
# Prints PASS/FAIL per assertion and exits non-zero on any failure.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIB="${SCRIPT_DIR}/../lib/inference_proxy.sh"

# shellcheck source=../lib/inference_proxy.sh
source "$LIB"

PASS=0
FAIL=0

assert_eq() {
  local name="$1" expected="$2" actual="$3"
  if [[ "$expected" == "$actual" ]]; then
    echo "  ✓ $name"
    PASS=$((PASS + 1))
  else
    echo "  ✗ $name"
    echo "      expected: $expected"
    echo "      actual:   $actual"
    FAIL=$((FAIL + 1))
  fi
}

assert_contains() {
  local name="$1" haystack="$2" needle="$3"
  if [[ "$haystack" == *"$needle"* ]]; then
    echo "  ✓ $name"
    PASS=$((PASS + 1))
  else
    echo "  ✗ $name"
    echo "      haystack: $haystack"
    echo "      needle:   $needle"
    FAIL=$((FAIL + 1))
  fi
}

assert_not_contains() {
  local name="$1" haystack="$2" needle="$3"
  if [[ "$haystack" != *"$needle"* ]]; then
    echo "  ✓ $name"
    PASS=$((PASS + 1))
  else
    echo "  ✗ $name"
    echo "      haystack: $haystack"
    echo "      should NOT contain: $needle"
    FAIL=$((FAIL + 1))
  fi
}

# Reset env state between cases so leftovers from a prior case can't leak.
reset_env() {
  unset SERA_GATEWAY_ENDPOINT SERA_LAUNCHER_TOKEN SERA_API_KEY \
        ANTHROPIC_API_KEY ANTHROPIC_AUTH_TOKEN ANTHROPIC_BASE_URL \
        OPENAI_API_KEY OPENAI_BASE_URL
  INFERENCE_PROXY_ARGV=()
  INFERENCE_PROXY_NONCOMPLIANT=""
}

# Override the reachability probe so we never hit a real network in tests.
inference_proxy_reachable() {
  [[ "${TEST_REACHABLE:-no}" == "yes" ]]
}

echo "── inference_proxy_endpoint ────────────────────────────────────────────"
reset_env
assert_eq "default endpoint" "http://localhost:42540" "$(inference_proxy_endpoint)"
SERA_GATEWAY_ENDPOINT="http://inference.local" \
  assert_eq "endpoint override" "http://inference.local" "$(SERA_GATEWAY_ENDPOINT=http://inference.local inference_proxy_endpoint)"

echo
echo "── inference_proxy_token ───────────────────────────────────────────────"
reset_env
assert_eq "default to dev bootstrap token" "sera_bootstrap_dev_123" "$(inference_proxy_token)"
SERA_API_KEY="abc-key" \
  assert_eq "SERA_API_KEY beats default" "abc-key" "$(SERA_API_KEY=abc-key inference_proxy_token)"
SERA_LAUNCHER_TOKEN="one-shot" SERA_API_KEY="abc-key" \
  assert_eq "SERA_LAUNCHER_TOKEN beats SERA_API_KEY" "one-shot" \
    "$(SERA_LAUNCHER_TOKEN=one-shot SERA_API_KEY=abc-key inference_proxy_token)"

echo
echo "── inference_proxy_build_argv: soft, reachable ─────────────────────────"
reset_env
TEST_REACHABLE=yes
inference_proxy_build_argv soft
rc=$?
assert_eq "soft+reachable returns 0" "0" "$rc"
assert_eq "compliant: NONCOMPLIANT flag empty" "" "$INFERENCE_PROXY_NONCOMPLIANT"
joined="${INFERENCE_PROXY_ARGV[*]}"
assert_contains "argv starts with env"             "$joined" "env "
assert_contains "argv scrubs ANTHROPIC_API_KEY"    "$joined" "-u ANTHROPIC_API_KEY"
assert_contains "argv scrubs ANTHROPIC_AUTH_TOKEN" "$joined" "-u ANTHROPIC_AUTH_TOKEN"
assert_contains "argv scrubs ANTHROPIC_BASE_URL"   "$joined" "-u ANTHROPIC_BASE_URL"
assert_contains "argv scrubs OPENAI_API_KEY"       "$joined" "-u OPENAI_API_KEY"
assert_contains "argv scrubs OPENAI_BASE_URL"      "$joined" "-u OPENAI_BASE_URL"
assert_contains "argv sets ANTHROPIC_BASE_URL to gateway" \
  "$joined" "ANTHROPIC_BASE_URL=http://localhost:42540"
assert_contains "argv sets OPENAI_BASE_URL to gateway/v1" \
  "$joined" "OPENAI_BASE_URL=http://localhost:42540/v1"
assert_contains "argv sets ANTHROPIC_API_KEY to SERA token" \
  "$joined" "ANTHROPIC_API_KEY=sera_bootstrap_dev_123"
assert_contains "argv sets ANTHROPIC_AUTH_TOKEN to SERA token" \
  "$joined" "ANTHROPIC_AUTH_TOKEN=sera_bootstrap_dev_123"
assert_contains "argv sets OPENAI_API_KEY to SERA token" \
  "$joined" "OPENAI_API_KEY=sera_bootstrap_dev_123"
assert_contains "argv tags SERA_LAUNCHER_PROXY for child visibility" \
  "$joined" "SERA_LAUNCHER_PROXY=http://localhost:42540"
assert_not_contains "compliant argv has no NONCOMPLIANT tag" \
  "$joined" "SERA_LAUNCHER_NONCOMPLIANT"

echo
echo "── inference_proxy_build_argv: strict, unreachable ─────────────────────"
reset_env
TEST_REACHABLE=no
inference_proxy_build_argv strict 2>/dev/null
rc=$?
assert_eq "strict+unreachable returns 2" "2" "$rc"
assert_eq "strict failure leaves argv empty" "0" "${#INFERENCE_PROXY_ARGV[@]}"

echo
echo "── inference_proxy_build_argv: strict, reachable ───────────────────────"
reset_env
TEST_REACHABLE=yes
inference_proxy_build_argv strict
rc=$?
assert_eq "strict+reachable returns 0" "0" "$rc"
assert_eq "strict success: NONCOMPLIANT empty" "" "$INFERENCE_PROXY_NONCOMPLIANT"
joined="${INFERENCE_PROXY_ARGV[*]}"
assert_contains "strict argv still scrubs OPENAI_API_KEY" "$joined" "-u OPENAI_API_KEY"

echo
echo "── inference_proxy_build_argv: soft, unreachable (degraded) ────────────"
reset_env
TEST_REACHABLE=no
inference_proxy_build_argv soft 2>/dev/null
rc=$?
assert_eq "soft+unreachable returns 0" "0" "$rc"
assert_eq "soft+unreachable marks NONCOMPLIANT" "yes" "$INFERENCE_PROXY_NONCOMPLIANT"
joined="${INFERENCE_PROXY_ARGV[*]}"
assert_contains "degraded argv still scrubs ANTHROPIC_API_KEY" \
  "$joined" "-u ANTHROPIC_API_KEY"
assert_contains "degraded argv tags SERA_LAUNCHER_NONCOMPLIANT=yes" \
  "$joined" "SERA_LAUNCHER_NONCOMPLIANT=yes"

echo
echo "── inference_proxy_build_argv: off ─────────────────────────────────────"
reset_env
TEST_REACHABLE=no
inference_proxy_build_argv off
rc=$?
assert_eq "off mode returns 0 even when unreachable" "0" "$rc"
assert_eq "off mode leaves argv empty" "0" "${#INFERENCE_PROXY_ARGV[@]}"

echo
echo "── token + endpoint propagation ────────────────────────────────────────"
reset_env
TEST_REACHABLE=yes
SERA_GATEWAY_ENDPOINT="http://inference.local"
SERA_LAUNCHER_TOKEN="sera-mint-9aa"
inference_proxy_build_argv soft
joined="${INFERENCE_PROXY_ARGV[*]}"
assert_contains "custom endpoint flows into ANTHROPIC_BASE_URL" \
  "$joined" "ANTHROPIC_BASE_URL=http://inference.local"
assert_contains "custom endpoint flows into OPENAI_BASE_URL" \
  "$joined" "OPENAI_BASE_URL=http://inference.local/v1"
assert_contains "custom token flows into ANTHROPIC_AUTH_TOKEN" \
  "$joined" "ANTHROPIC_AUTH_TOKEN=sera-mint-9aa"
assert_contains "custom token flows into OPENAI_API_KEY" \
  "$joined" "OPENAI_API_KEY=sera-mint-9aa"
# Belt-and-braces: prove the operator's real provider key is never echoed.
assert_not_contains "no leaked operator anthropic key in argv" \
  "$joined" "sk-ant-"
assert_not_contains "no leaked operator openai key in argv" \
  "$joined" "sk-proj-"

echo
echo "── env scrubbing actually wipes inherited child env ────────────────────"
# Belt-and-braces: prove that running `env ${INFERENCE_PROXY_ARGV[@]} -- env`
# really does drop the operator's keys before the child sees them.
reset_env
TEST_REACHABLE=yes
ANTHROPIC_API_KEY="sk-ant-leaked-OPERATOR-KEY"
OPENAI_API_KEY="sk-proj-leaked-OPERATOR-KEY"
inference_proxy_build_argv soft
child_env="$("${INFERENCE_PROXY_ARGV[@]}" env)"
assert_not_contains "operator ANTHROPIC_API_KEY value not visible to child" \
  "$child_env" "sk-ant-leaked-OPERATOR-KEY"
assert_not_contains "operator OPENAI_API_KEY value not visible to child" \
  "$child_env" "sk-proj-leaked-OPERATOR-KEY"
assert_contains "child sees SERA-injected ANTHROPIC_BASE_URL" \
  "$child_env" "ANTHROPIC_BASE_URL=http://localhost:42540"
assert_contains "child sees SERA-injected OPENAI_BASE_URL" \
  "$child_env" "OPENAI_BASE_URL=http://localhost:42540/v1"

echo
echo "──────────────────────────────────────────────────────────────────"
echo "  Passed: $PASS"
echo "  Failed: $FAIL"
echo "──────────────────────────────────────────────────────────────────"

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
exit 0
