#!/usr/bin/env bash
# scripts/lib/inference_proxy.sh — Pattern B launcher enforcement (sera-kg3g)
#
# Builds the env-prefix needed to route an OMC/OMX child through the SERA
# gateway's inference.local proxy (`/v1/messages`, `/v1/chat/completions`)
# instead of the operator's direct Anthropic/OpenAI accounts.
#
# Public API (all functions are pure or read env vars; none modify global env):
#
#   inference_proxy_endpoint
#       Echo the gateway endpoint to use. Reads $SERA_GATEWAY_ENDPOINT
#       (default: http://localhost:42540).
#
#   inference_proxy_token
#       Echo the SERA-issued token to inject as the child's upstream key.
#       Reads, in priority order:
#         $SERA_LAUNCHER_TOKEN  — explicit one-shot token
#         $SERA_API_KEY         — gateway-configured API key
#         "sera_bootstrap_dev_123" — dev fallback (matches doctor.rs default)
#
#   inference_proxy_reachable <endpoint> [timeout_seconds]
#       Return 0 if `${endpoint}/api/health` responds with HTTP 200.
#       Default timeout: 2 seconds. The function may be overridden in tests
#       by redefining it after sourcing.
#
#   inference_proxy_build_argv <mode>
#       Populate global array INFERENCE_PROXY_ARGV with `env ... --` prefix
#       args to splice in front of the child command. Sets global flag
#       INFERENCE_PROXY_NONCOMPLIANT to "yes" when soft mode is degrading.
#
#       Modes:
#         strict — fail (return 2) if the gateway is unreachable.
#         soft   — warn-and-continue, mark non-compliant.
#         off    — leave INFERENCE_PROXY_ARGV empty and return 0.
#
#       Returns:
#         0 — argv built (compliant or soft-non-compliant); proceed.
#         2 — strict mode but gateway unreachable; caller must abort.
#
# Provider env vars handled (scrubbed via `env -u` so the child does not
# inherit the operator's direct-provider credentials):
#   ANTHROPIC_API_KEY      — replaced with SERA token (kept set so SDK runs)
#   ANTHROPIC_AUTH_TOKEN   — replaced with SERA token (Authorization: Bearer)
#   ANTHROPIC_BASE_URL     — replaced with gateway endpoint
#   OPENAI_API_KEY         — replaced with SERA token
#   OPENAI_BASE_URL        — replaced with gateway endpoint + /v1

# shellcheck shell=bash

# Globals populated by inference_proxy_build_argv.
INFERENCE_PROXY_ARGV=()
INFERENCE_PROXY_NONCOMPLIANT=""

inference_proxy_endpoint() {
  echo "${SERA_GATEWAY_ENDPOINT:-http://localhost:42540}"
}

inference_proxy_token() {
  if [[ -n "${SERA_LAUNCHER_TOKEN:-}" ]]; then
    echo "${SERA_LAUNCHER_TOKEN}"
  elif [[ -n "${SERA_API_KEY:-}" ]]; then
    echo "${SERA_API_KEY}"
  else
    echo "sera_bootstrap_dev_123"
  fi
}

# Reachability probe — overridable in tests.
inference_proxy_reachable() {
  local endpoint="$1"
  local timeout="${2:-2}"
  local code
  code=$(curl -s -o /dev/null -w "%{http_code}" \
              --max-time "${timeout}" \
              "${endpoint%/}/api/health" 2>/dev/null || echo "000")
  [[ "$code" == "200" ]]
}

inference_proxy_build_argv() {
  local mode="${1:-soft}"
  INFERENCE_PROXY_ARGV=()
  INFERENCE_PROXY_NONCOMPLIANT=""

  if [[ "$mode" == "off" ]]; then
    return 0
  fi

  local endpoint token openai_base
  endpoint="$(inference_proxy_endpoint)"
  token="$(inference_proxy_token)"
  openai_base="${endpoint%/}/v1"

  if ! inference_proxy_reachable "$endpoint"; then
    case "$mode" in
      strict)
        echo "[sera-launcher] FATAL: gateway unreachable at ${endpoint}; refusing to launch (strict mode)" >&2
        echo "[sera-launcher] Start the gateway (e.g. scripts/sera-local) or set SERA_INFERENCE_PROXY_MODE=soft to override." >&2
        return 2
        ;;
      soft|*)
        INFERENCE_PROXY_NONCOMPLIANT="yes"
        echo "[sera-launcher] WARN: gateway unreachable at ${endpoint}; session marked NON-COMPLIANT (provider keys remain scrubbed)" >&2
        ;;
    esac
  fi

  # `env` builds the child's environment: scrub the operator's direct-provider
  # vars first (-u) so the child cannot inherit a stale key, then set our own
  # base URL + SERA token so the child can talk to the gateway proxy.
  INFERENCE_PROXY_ARGV=(
    env
    -u ANTHROPIC_API_KEY
    -u ANTHROPIC_AUTH_TOKEN
    -u ANTHROPIC_BASE_URL
    -u OPENAI_API_KEY
    -u OPENAI_BASE_URL
    "ANTHROPIC_BASE_URL=${endpoint}"
    "ANTHROPIC_API_KEY=${token}"
    "ANTHROPIC_AUTH_TOKEN=${token}"
    "OPENAI_BASE_URL=${openai_base}"
    "OPENAI_API_KEY=${token}"
    "SERA_LAUNCHER_PROXY=${endpoint}"
  )

  if [[ -n "$INFERENCE_PROXY_NONCOMPLIANT" ]]; then
    INFERENCE_PROXY_ARGV+=("SERA_LAUNCHER_NONCOMPLIANT=yes")
  fi

  return 0
}
