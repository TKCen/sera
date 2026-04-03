#!/bin/sh
# Minimal BYOH Shell Agent — SERA contract reference implementation.
#
# Implements the SERA BYOH contract (docs/BYOH-CONTRACT.md) for ephemeral mode:
#   - Health server on AGENT_CHAT_PORT via socat (§5)
#   - Task read from stdin as JSON (§1)
#   - LLM call via SERA_LLM_PROXY_URL using curl (§3)
#   - Result written to stdout as JSON (§2)
#   - SIGTERM trap for graceful shutdown (§7)

set -e

CHAT_PORT="${AGENT_CHAT_PORT:-3100}"
LLM_URL="${SERA_LLM_PROXY_URL}"
TOKEN="${SERA_IDENTITY_TOKEN}"

# ---------------------------------------------------------------------------
# §7 Graceful shutdown — trap SIGTERM before doing any work
# ---------------------------------------------------------------------------
HEALTH_PID=""

cleanup() {
    echo "[byoh-shell] SIGTERM received — shutting down" >&2
    if [ -n "$HEALTH_PID" ]; then
        kill "$HEALTH_PID" 2>/dev/null || true
    fi
    exit 0
}

trap cleanup TERM INT

# ---------------------------------------------------------------------------
# §5 Health server via socat
# The loop restarts socat for each connection so it handles multiple polls.
# ready/busy state is communicated via a temp file to the subshell.
# ---------------------------------------------------------------------------
STATE_FILE="$(mktemp /tmp/byoh-state.XXXXXX)"
echo '{"ready":false,"busy":false}' > "$STATE_FILE"

health_server() {
    while true; do
        # socat listens for one TCP connection, forks a child, reads the HTTP
        # request, and responds with the current state JSON.
        socat -d TCP-LISTEN:"$CHAT_PORT",reuseaddr,fork SYSTEM:'
            read line
            # Read and discard remaining headers until blank line
            while IFS= read -r h && [ "$(printf "%s" "$h" | tr -d "\r\n")" != "" ]; do :; done
            STATE=$(cat '"$STATE_FILE"')
            BODY=$(printf "%s" "$STATE")
            LEN=$(printf "%s" "$BODY" | wc -c | tr -d " ")
            printf "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: %s\r\n\r\n%s" "$LEN" "$BODY"
        ' 2>/dev/null &
        SOCAT_PID=$!
        # Wait briefly; if socat exits (e.g. connection handled), loop again
        sleep 1
        # Kill leftover socat parent between loops (children already forked)
        kill "$SOCAT_PID" 2>/dev/null || true
    done
}

health_server &
HEALTH_PID=$!
echo "[byoh-shell] health server listening on :$CHAT_PORT (pid $HEALTH_PID)" >&2

# Mark ready
echo '{"ready":true,"busy":false}' > "$STATE_FILE"

# ---------------------------------------------------------------------------
# §1 Read task from stdin (one JSON line)
# ---------------------------------------------------------------------------
RAW="$(cat)"
if [ -z "$RAW" ]; then
    printf '{"taskId":"unknown","result":null,"error":"empty stdin"}\n'
    exit 1
fi

TASK_ID="$(printf '%s' "$RAW" | jq -r '.taskId')"
TASK="$(printf '%s' "$RAW" | jq -r '.task')"
CONTEXT="$(printf '%s' "$RAW" | jq -r '.context // ""')"

echo "[byoh-shell] received task '$TASK_ID'" >&2

# Mark busy
echo '{"ready":true,"busy":true}' > "$STATE_FILE"

# ---------------------------------------------------------------------------
# §3 LLM call via SERA proxy using curl
# Build prompt — append context if present
# ---------------------------------------------------------------------------
if [ -n "$CONTEXT" ]; then
    PROMPT="${TASK}

Context: ${CONTEXT}"
else
    PROMPT="$TASK"
fi

# Escape prompt for JSON embedding
PROMPT_JSON="$(printf '%s' "$PROMPT" | jq -Rs '.')"

REQUEST_BODY="$(printf '{
  "model": "gpt-4o-mini",
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": %s}
  ],
  "max_tokens": 1024
}' "$PROMPT_JSON")"

RESULT=""
ERROR=""

HTTP_RESPONSE="$(curl -s -w "\n%{http_code}" \
    -X POST "${LLM_URL}/chat/completions" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    -d "$REQUEST_BODY" \
    --max-time 60 2>/dev/null)" || true

HTTP_STATUS="$(printf '%s' "$HTTP_RESPONSE" | tail -n1)"
HTTP_BODY="$(printf '%s' "$HTTP_RESPONSE" | head -n -1)"

if [ "$HTTP_STATUS" = "429" ]; then
    ERROR="Token budget exhausted (429). Task cannot proceed."
    echo "[byoh-shell] $ERROR" >&2
elif [ "$HTTP_STATUS" = "200" ]; then
    RESULT="$(printf '%s' "$HTTP_BODY" | jq -r '.choices[0].message.content // empty' 2>/dev/null)" || true
    if [ -z "$RESULT" ]; then
        ERROR="LLM returned empty content"
    fi
    echo "[byoh-shell] task '$TASK_ID' completed" >&2
else
    ERROR="LLM proxy error: HTTP $HTTP_STATUS"
    echo "[byoh-shell] $ERROR" >&2
fi

# Mark not busy
echo '{"ready":true,"busy":false}' > "$STATE_FILE"

# ---------------------------------------------------------------------------
# §2 Write result to stdout — the ONLY JSON written to stdout
# ---------------------------------------------------------------------------
if [ -n "$ERROR" ]; then
    printf '%s\n' "$(jq -n \
        --arg taskId "$TASK_ID" \
        --argjson result null \
        --arg error "$ERROR" \
        '{"taskId":$taskId,"result":$result,"error":$error}')"
else
    printf '%s\n' "$(jq -n \
        --arg taskId "$TASK_ID" \
        --arg result "$RESULT" \
        --argjson error null \
        '{"taskId":$taskId,"result":$result,"error":$error}')"
fi

# Cleanup and exit
rm -f "$STATE_FILE"
kill "$HEALTH_PID" 2>/dev/null || true
exit 0
