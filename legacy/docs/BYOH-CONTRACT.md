# BYOH Contract — Bring Your Own Harness

This document defines the interface that **any** agent harness must implement to run inside a SERA container. If your harness satisfies every dimension below, SERA will orchestrate it correctly regardless of language, framework, or internal architecture.

---

## Overview

SERA is harness-agnostic. The built-in harness (`core/agent-runtime/`) is the reference implementation, but you can replace it entirely. SERA communicates with your container through:

- **Environment variables** — configuration injected at container start
- **HTTP endpoints** — health, status, and optional chat
- **stdin/stdout** — ephemeral task I/O
- **REST API** — persistent task polling and result reporting
- **Signals** — graceful shutdown via SIGTERM

---

## 1. Task Receive

How your harness receives work depends on `AGENT_LIFECYCLE_MODE`.

### Ephemeral Mode

SERA writes a single JSON object to **stdin** immediately after container start:

```json
{
  "taskId": "task-abc123",
  "task": "Summarize the README.md file",
  "context": "The file is in /workspace/README.md"
}
```

| Field     | Type   | Required | Description                 |
| --------- | ------ | -------- | --------------------------- |
| `taskId`  | string | yes      | Unique task identifier      |
| `task`    | string | yes      | Task description or prompt  |
| `context` | string | no       | Optional additional context |

Your harness reads this from stdin, executes the task, writes the result to stdout (see §2), and exits. SERA does not send a second task.

### Persistent Mode

Your harness polls for the next available task:

```
GET ${SERA_CORE_URL}/api/agents/${AGENT_INSTANCE_ID}/tasks/next
Authorization: Bearer ${SERA_IDENTITY_TOKEN}
```

**Response when a task is available (200):**

```json
{
  "taskId": "task-abc123",
  "task": "Summarize the README.md file",
  "context": "The file is in /workspace/README.md"
}
```

**Response when no task is queued (204):** empty body — poll again after a backoff interval.

Recommended poll interval: 2–5 seconds with exponential backoff on consecutive 204 responses.

---

## 2. Result Report

### Ephemeral Mode

Write a single JSON object to **stdout** before exiting:

```json
{
  "taskId": "task-abc123",
  "result": "The README describes a Docker-native multi-agent AI platform.",
  "error": null
}
```

| Field    | Type           | Required | Description                                     |
| -------- | -------------- | -------- | ----------------------------------------------- |
| `taskId` | string         | yes      | Must match the received `taskId`                |
| `result` | string \| null | yes      | Task output; `null` on failure                  |
| `error`  | string         | no       | Human-readable error message if the task failed |

Write only valid JSON to stdout. Log all debug output to **stderr**.

### Persistent Mode

POST to the completion endpoint after finishing a task:

```
POST ${SERA_CORE_URL}/api/agents/${AGENT_INSTANCE_ID}/tasks/${taskId}/complete
Authorization: Bearer ${SERA_IDENTITY_TOKEN}
Content-Type: application/json

{
  "taskId": "task-abc123",
  "result": "The README describes a Docker-native multi-agent AI platform.",
  "error": null
}
```

**Success response (200):** `{ "ok": true }`

After reporting, poll for the next task immediately.

---

## 3. LLM Access

All LLM calls are routed through SERA's in-process proxy. This allows sera-core to meter token usage, enforce budgets, and apply egress policies transparently.

```
POST ${SERA_LLM_PROXY_URL}/chat/completions
Authorization: Bearer ${SERA_IDENTITY_TOKEN}
Content-Type: application/json
```

**Request body** — standard OpenAI Chat Completions format:

```json
{
  "model": "gpt-4o",
  "messages": [
    { "role": "system", "content": "You are a helpful assistant." },
    { "role": "user", "content": "Summarize the README." }
  ],
  "temperature": 0.7,
  "max_tokens": 1024
}
```

**Response** — standard OpenAI Chat Completions response:

```json
{
  "id": "chatcmpl-...",
  "object": "chat.completion",
  "choices": [
    {
      "index": 0,
      "message": { "role": "assistant", "content": "..." },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 42,
    "completion_tokens": 128,
    "total_tokens": 170
  }
}
```

Streaming (`"stream": true`) is supported and returns standard SSE chunks.

**Important:**

- Token usage is metered and budget-enforced by `MeteringService` in sera-core automatically — your harness does not need to track tokens.
- If the agent's token budget is exhausted, the proxy returns **429 Too Many Requests**. Your harness should surface this as a task error rather than retrying indefinitely.
- The model name in the request must match a model registered in sera-core's `providers.json`.

---

## 4. Security Inheritance

### Egress Proxy _(Phase 2 — not yet injected)_

> **Status:** `HTTP_PROXY`, `HTTPS_PROXY`, and `NO_PROXY` are **not currently injected** by `SandboxManager`. This is planned for Phase 2.

When the egress proxy is enabled in a future release, SERA will inject:

```
HTTP_PROXY=http://sera-egress-proxy:3128
HTTPS_PROXY=http://sera-egress-proxy:3128
NO_PROXY=sera-core,centrifugo,localhost,127.0.0.1
```

Most HTTP client libraries respect these variables automatically (Python `requests`, Node.js `got`/`axios` with proxy agent, Go `net/http`, curl, wget). Your harness will not need explicit proxy configuration if it uses a standard HTTP client.

**`NO_PROXY` exclusions** will prevent proxy routing for:

- `sera-core` — direct communication with the SERA API
- `centrifugo` — real-time message bus
- `localhost` / `127.0.0.1` — loopback traffic

### Filesystem Isolation

- Your harness has write access to `/workspace` (bind-mounted from the host)
- Do not assume write access outside `/workspace`
- `/tmp` is available for temporary scratch space

### Resource Limits

CPU and memory limits are enforced by Docker via the `SandboxBoundary` tier assigned to the agent template:

| Tier   | CPU        | Memory |
| ------ | ---------- | ------ |
| tier-1 | 0.25 cores | 256 MB |
| tier-2 | 1 core     | 1 GB   |
| tier-3 | 4 cores    | 4 GB   |

These limits are applied to the container's Docker `HostConfig` at creation time and cannot be bypassed from within the container.

### Proxy Enforcement Notes _(Phase 2)_

> **Not yet active.** `HTTP_PROXY`/`HTTPS_PROXY` injection and iptables enforcement are both Phase 2 work.
>
> When implemented, enforcement will vary by deployment:
>
> - **Docker Desktop (Windows/macOS):** iptables enforcement is not feasible. Proxy compliance will be advisory only.
> - **Linux with Docker CE:** iptables rules on `agent_net` can DROP non-proxy outbound traffic, providing hard enforcement. Production deployments should use Linux with iptables enforcement enabled.

---

## 5. Health / Status

Your container **MUST** expose an HTTP server on `AGENT_CHAT_PORT` (default `3100`).

### Required: GET /health

```
GET http://localhost:${AGENT_CHAT_PORT}/health
```

**Response (200):**

```json
{
  "ready": true,
  "busy": false
}
```

| Field   | Type    | Description                                                            |
| ------- | ------- | ---------------------------------------------------------------------- |
| `ready` | boolean | `true` when the harness has finished initialising and can accept tasks |
| `busy`  | boolean | `true` while a task is actively being executed                         |

SERA polls this endpoint with exponential backoff after container start. The instance is not marked `running` until `ready: true` is returned. Return `ready: false` during model loading, warmup, or any initialisation that must complete before tasks are accepted.

### Optional: POST /chat

Implement this endpoint to enable interactive web chat from the SERA dashboard.

```
POST http://localhost:${AGENT_CHAT_PORT}/chat
Content-Type: application/json
```

**Request body:**

```json
{
  "message": "What files are in /workspace?",
  "sessionId": "session-xyz789",
  "history": [
    { "role": "user", "content": "Hello" },
    { "role": "assistant", "content": "Hi! How can I help?" }
  ],
  "messageId": "msg-001"
}
```

| Field       | Type          | Required | Description                     |
| ----------- | ------------- | -------- | ------------------------------- |
| `message`   | string        | yes      | The user's current message      |
| `sessionId` | string        | yes      | Conversation session identifier |
| `history`   | ChatMessage[] | no       | Prior conversation turns        |
| `messageId` | string        | no       | Idempotency key for the message |

**Response (200):**

```json
{
  "result": "The /workspace directory contains: README.md, main.py",
  "error": null
}
```

| Field    | Type           | Required | Description                         |
| -------- | -------------- | -------- | ----------------------------------- |
| `result` | string \| null | yes      | Assistant response; `null` on error |
| `error`  | string         | no       | Error message if the request failed |

If `/chat` is not implemented, the SERA dashboard will show the agent as non-interactive but will still orchestrate task execution normally.

---

## 6. Heartbeat

Persistent-mode containers **SHOULD** send periodic heartbeats to signal liveness:

```
POST ${SERA_CORE_URL}/api/agents/${AGENT_INSTANCE_ID}/heartbeat
Authorization: Bearer ${SERA_IDENTITY_TOKEN}
Content-Type: application/json

{}
```

**Response (200):** `{ "ok": true }`

- Send at the interval defined by `AGENT_HEARTBEAT_INTERVAL_MS` (default: `30000` ms)
- Send immediately on startup, then on the configured interval
- An instance that misses 3 consecutive heartbeat windows is marked `unresponsive` by sera-core's health monitor
- An `unresponsive` instance is not sent new tasks until it resumes heartbeating

Ephemeral containers do not need to heartbeat — they complete and exit before the first heartbeat window.

---

## 7. Graceful Shutdown

Your container **MUST** handle `SIGTERM`.

SERA sends `SIGTERM` when stopping an instance (manual stop, scale-down, or container replacement). The shutdown sequence is:

1. SERA sends `SIGTERM` to the container's main process
2. Your harness has **30 seconds** to flush state and exit cleanly
3. After 30 seconds, SERA sends `SIGKILL`

**What to do on SIGTERM:**

- Stop accepting new tasks (persistent mode: stop polling)
- Allow any in-flight task to complete if it will finish within the window, otherwise write a partial result with an appropriate error
- Close open file handles and database connections
- Exit with code `0` on clean shutdown, non-zero on error

If your process is a shell script or wrapper, ensure SIGTERM is forwarded to child processes (use `exec` for the final command, or install an explicit signal handler).

---

## Injected Environment Variables

### Always injected

These variables are injected by `SandboxManager` at container start for every agent instance.

| Variable                      | Description                                                                       | Example                            |
| ----------------------------- | --------------------------------------------------------------------------------- | ---------------------------------- |
| `SERA_CORE_URL`               | Base URL for sera-core API                                                        | `http://sera-core:3001`            |
| `SERA_LLM_PROXY_URL`          | Full URL for the LLM proxy endpoint                                               | `http://sera-core:3001/v1/llm`     |
| `AGENT_NAME`                  | Template or manifest name                                                         | `my-summariser`                    |
| `AGENT_INSTANCE_ID`           | Unique instance UUID                                                              | `inst-4f2a9c1e`                    |
| `AGENT_CHAT_PORT`             | Port your health/chat HTTP server must bind to (from `spec.sandbox.chatPort`)     | `3100`                             |
| `AGENT_HEARTBEAT_INTERVAL_MS` | Heartbeat interval in milliseconds                                                | `30000`                            |
| `AGENT_LIFECYCLE_MODE`        | `persistent` or `ephemeral`                                                       | `ephemeral`                        |
| `CENTRIFUGO_API_URL`          | Internal Centrifugo API URL (used by agent-runtime for real-time events)          | `http://centrifugo:8000/api`       |
| `CENTRIFUGO_API_KEY`          | Centrifugo API key                                                                | _(set via server env)_             |

### Conditionally injected

| Variable                | Condition                                             | Description                                              |
| ----------------------- | ----------------------------------------------------- | -------------------------------------------------------- |
| `SERA_IDENTITY_TOKEN`   | Only when `request.token` is present                  | JWT for authenticating with sera-core API and LLM proxy  |
| `SERA_SKILLS_DIR`       | Only when skill packages are granted to the agent     | Mount path for pre-loaded skill packages (`/sera/skills`) |
| `SERA_SECRET_<NAME>`    | One per agent-env secret granted to the instance      | Decrypted secret value; `NAME` is uppercased             |

### Phase 2 — not yet injected

These variables are planned but **not currently set** by `SandboxManager`:

| Variable      | Planned Purpose                                        |
| ------------- | ------------------------------------------------------ |
| `HTTP_PROXY`  | Route outbound HTTP through the Squid egress proxy     |
| `HTTPS_PROXY` | Route outbound HTTPS through the Squid egress proxy    |
| `NO_PROXY`    | Exclude internal hostnames from proxy routing          |

---

## Reserved Environment Variables

These variables are reserved for future use and must not be used for other purposes:

| Variable                  | Reserved For                                                          |
| ------------------------- | --------------------------------------------------------------------- |
| `SERA_CENTRIFUGO_URL`     | v2 observability sidecar — thought streaming and real-time event push |
| `SERA_CENTRIFUGO_CHANNEL` | v2 thought streaming channel name for this agent instance             |

---

## Quick Start Example

The following minimal Python harness satisfies the contract for **ephemeral** mode:

```python
#!/usr/bin/env python3
"""
Minimal BYOH-compliant ephemeral harness.
Reads a task from stdin, calls the LLM proxy, writes the result to stdout.
"""

import os
import json
import sys
import threading
from http.server import HTTPServer, BaseHTTPRequestHandler

import requests

# --- Read env vars ---
SERA_CORE_URL         = os.environ["SERA_CORE_URL"]
SERA_LLM_PROXY_URL    = os.environ["SERA_LLM_PROXY_URL"]
AGENT_CHAT_PORT       = int(os.environ.get("AGENT_CHAT_PORT", "3100"))
# SERA_IDENTITY_TOKEN is injected only when a token is present on the spawn request.
# Agents that always run with a token can use os.environ["SERA_IDENTITY_TOKEN"].
SERA_IDENTITY_TOKEN   = os.environ.get("SERA_IDENTITY_TOKEN", "")

_ready = False
_busy  = False


# --- Health endpoint (required) ---
class HealthHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            body = json.dumps({"ready": _ready, "busy": _busy}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, *args):
        pass  # suppress request logs; use stderr for debug output


def start_health_server():
    server = HTTPServer(("0.0.0.0", AGENT_CHAT_PORT), HealthHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server


# --- LLM call ---
def call_llm(prompt: str) -> str:
    headers = {
        "Authorization": f"Bearer {SERA_IDENTITY_TOKEN}",
        "Content-Type": "application/json",
    }
    payload = {
        "model": "gpt-4o-mini",
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": 1024,
    }
    resp = requests.post(
        f"{SERA_LLM_PROXY_URL}/chat/completions",
        json=payload,
        headers=headers,
        timeout=60,
    )
    resp.raise_for_status()
    return resp.json()["choices"][0]["message"]["content"]


# --- Main ---
def main():
    global _ready, _busy

    start_health_server()
    _ready = True

    # Read task from stdin
    raw = sys.stdin.read()
    task_input = json.loads(raw)
    task_id = task_input["taskId"]
    task    = task_input["task"]
    context = task_input.get("context", "")

    _busy = True
    result = None
    error  = None

    try:
        prompt = f"{task}\n\nContext: {context}" if context else task
        result = call_llm(prompt)
    except requests.HTTPError as exc:
        error = f"LLM proxy error: {exc.response.status_code} {exc.response.text}"
    except Exception as exc:
        error = str(exc)
    finally:
        _busy = False

    # Write result to stdout (the ONLY JSON written to stdout)
    output = {"taskId": task_id, "result": result, "error": error}
    print(json.dumps(output))


if __name__ == "__main__":
    main()
```

**Key points illustrated:**

1. Health server starts before `_ready = True` so SERA's startup poll sees `ready: false` during init.
2. All debug/log output goes to `stderr` — `stdout` carries only the final JSON result.
3. LLM calls go to `SERA_LLM_PROXY_URL` with the identity token — no direct provider API keys needed.
4. `error` is always included in the output (even as `null`) so SERA can distinguish success from failure.

For a persistent-mode harness, replace stdin/stdout I/O with the polling loop described in §1 and §2, and add the heartbeat loop from §6.

---

## Compliance Checklist

Use this checklist to verify your harness before deploying to SERA:

- [ ] Reads `AGENT_LIFECYCLE_MODE` and selects correct task receive/report path
- [ ] **Ephemeral:** reads task from stdin as JSON, writes result to stdout as JSON
- [ ] **Persistent:** polls `GET .../tasks/next`, POSTs to `.../tasks/:id/complete`
- [ ] Routes LLM calls to `SERA_LLM_PROXY_URL` with `Authorization: Bearer ${SERA_IDENTITY_TOKEN}`
- [ ] Starts HTTP server on `AGENT_CHAT_PORT` before setting `ready: true`
- [ ] `GET /health` returns `{ "ready": boolean, "busy": boolean }`
- [ ] **Persistent:** sends heartbeat POSTs at `AGENT_HEARTBEAT_INTERVAL_MS`
- [ ] Handles `SIGTERM` and exits within 30 seconds
- [ ] Writes only valid JSON to stdout; all other output goes to stderr
- [ ] Does not include secrets or API keys — uses `SERA_IDENTITY_TOKEN` for all authenticated calls

---

## See Also

- `docs/ARCHITECTURE.md` — SERA system architecture and data models
- `core/agent-runtime/` — Reference harness implementation (TypeScript/Bun)
- `schemas/byoh-*.schema.json` — JSON Schema definitions for all contract payloads
- `docs/epics/` — Epic specs that define acceptance criteria for agent capabilities
