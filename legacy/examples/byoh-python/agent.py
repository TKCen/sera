#!/usr/bin/env python3
"""Minimal BYOH Python Agent — SERA contract reference implementation.

Implements the SERA BYOH contract (docs/BYOH-CONTRACT.md) for ephemeral mode:
  - Health server on AGENT_CHAT_PORT (§5)
  - Task read from stdin as JSON (§1)
  - LLM call via SERA_LLM_PROXY_URL (§3)
  - Result written to stdout as JSON (§2)
  - SIGTERM handling for graceful shutdown (§7)
"""

import json
import os
import signal
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

import requests

# ---------------------------------------------------------------------------
# Mandatory env vars (SERA injects these at container start)
# ---------------------------------------------------------------------------
SERA_CORE_URL = os.environ.get("SERA_CORE_URL", "http://sera-core:3001")
SERA_IDENTITY_TOKEN = os.environ["SERA_IDENTITY_TOKEN"]
SERA_LLM_PROXY_URL = os.environ["SERA_LLM_PROXY_URL"]
AGENT_CHAT_PORT = int(os.environ.get("AGENT_CHAT_PORT", "3100"))
AGENT_LIFECYCLE_MODE = os.environ.get("AGENT_LIFECYCLE_MODE", "ephemeral")

# ---------------------------------------------------------------------------
# Shared state — read by the health handler from the main thread
# ---------------------------------------------------------------------------
_ready = False
_busy = False
_shutdown = threading.Event()


# ---------------------------------------------------------------------------
# §5 Health endpoint
# ---------------------------------------------------------------------------
class HealthHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
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

    def log_message(self, fmt: str, *args: object) -> None:  # type: ignore[override]
        # Suppress per-request access logs; use stderr for debug output.
        pass


def start_health_server() -> HTTPServer:
    server = HTTPServer(("0.0.0.0", AGENT_CHAT_PORT), HealthHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    print(f"[byoh-python] health server listening on :{AGENT_CHAT_PORT}", file=sys.stderr)
    return server


# ---------------------------------------------------------------------------
# §3 LLM access via SERA proxy
# ---------------------------------------------------------------------------
def call_llm(prompt: str) -> str:
    """Call the LLM proxy with a simple user prompt and return the text reply."""
    headers = {
        "Authorization": f"Bearer {SERA_IDENTITY_TOKEN}",
        "Content-Type": "application/json",
    }
    payload = {
        "model": "gpt-4o-mini",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": prompt},
        ],
        "max_tokens": 1024,
    }
    resp = requests.post(
        f"{SERA_LLM_PROXY_URL}/chat/completions",
        json=payload,
        headers=headers,
        timeout=60,
    )
    if resp.status_code == 429:
        raise RuntimeError("Token budget exhausted (429). Task cannot proceed.")
    resp.raise_for_status()
    return resp.json()["choices"][0]["message"]["content"]


# ---------------------------------------------------------------------------
# §7 Graceful shutdown
# ---------------------------------------------------------------------------
def handle_sigterm(signum: int, frame: object) -> None:
    print("[byoh-python] SIGTERM received — shutting down", file=sys.stderr)
    _shutdown.set()


# ---------------------------------------------------------------------------
# Main — ephemeral task loop
# ---------------------------------------------------------------------------
def main() -> None:
    global _ready, _busy

    signal.signal(signal.SIGTERM, handle_sigterm)

    # Start health server BEFORE marking ready so SERA's startup poll sees
    # ready=false during init and only proceeds once we're truly ready.
    server = start_health_server()
    _ready = True

    if AGENT_LIFECYCLE_MODE != "ephemeral":
        # Persistent mode is not implemented in this minimal example.
        # See docs/BYOH-CONTRACT.md §1/§2/§6 for the polling + heartbeat loop.
        print("[byoh-python] only ephemeral mode is implemented in this example", file=sys.stderr)
        sys.exit(1)

    # §1 Read task from stdin (one JSON line)
    raw = sys.stdin.read().strip()
    if not raw:
        print(json.dumps({"taskId": "unknown", "result": None, "error": "empty stdin"}))
        sys.exit(1)

    task_input = json.loads(raw)
    task_id: str = task_input["taskId"]
    task: str = task_input["task"]
    context: str = task_input.get("context", "")

    print(f"[byoh-python] received task {task_id!r}", file=sys.stderr)

    _busy = True
    result = None
    error = None

    try:
        prompt = f"{task}\n\nContext: {context}" if context else task
        result = call_llm(prompt)
        print(f"[byoh-python] task {task_id!r} completed", file=sys.stderr)
    except requests.HTTPError as exc:
        error = f"LLM proxy error: {exc.response.status_code} {exc.response.text[:200]}"
        print(f"[byoh-python] {error}", file=sys.stderr)
    except Exception as exc:
        error = str(exc)
        print(f"[byoh-python] task failed: {error}", file=sys.stderr)
    finally:
        _busy = False

    # §2 Write result to stdout — this is the ONLY JSON written to stdout.
    output = {"taskId": task_id, "result": result, "error": error}
    print(json.dumps(output))
    sys.stdout.flush()

    server.shutdown()


if __name__ == "__main__":
    main()
