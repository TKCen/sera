#!/usr/bin/env python3
"""Strict operator-task live smoke for the SERA gateway.

This script is intentionally conservative: it can validate a captured smoke JSON
without touching a live service, or run the same checks against a local gateway.
It fails if readiness is not connected or if an operator-task result contains an
interrupted/doom-loop sentinel that would make a false-green closeout.
"""

import argparse
import json
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from typing import Any

DEFAULT_BASE = "http://127.0.0.1:3001"
DEFAULT_OUT = "/tmp/sera-6zcb-live-smoke.json"
DEFAULT_CONTAINER = "rust-sera-1"


def is_interrupted_runtime_result(value: Any) -> bool:
    if not isinstance(value, str):
        return False
    lowered = value.lower()
    return "[interrupted:" in lowered or ("interrupted" in lowered and "doom loop:" in lowered)


def container_api_key(container: str) -> str:
    raw = subprocess.check_output(["docker", "inspect", container], text=True)
    data = json.loads(raw)[0]
    for env in data.get("Config", {}).get("Env", []):
        if env.startswith("SERA_API_KEY="):
            return env.split("=", 1)[1]
    return ""


def req(base: str, auth_headers: dict[str, str], method: str, path: str, body: Any = None, timeout: int = 30) -> dict[str, Any]:
    data = None
    headers = dict(auth_headers)
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(base + path, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as resp:
            raw = resp.read().decode("utf-8", "replace")
            ctype = resp.headers.get("content-type", "")
            parsed = None
            if "json" in ctype or raw.startswith("{") or raw.startswith("["):
                try:
                    parsed = json.loads(raw)
                except Exception:
                    parsed = None
            return {"ok": True, "status": resp.status, "raw": raw, "json": parsed}
    except urllib.error.HTTPError as exc:
        raw = exc.read().decode("utf-8", "replace")
        parsed = None
        try:
            parsed = json.loads(raw)
        except Exception:
            pass
        return {"ok": False, "status": exc.code, "raw": raw, "json": parsed}


def sse_reader(base: str, auth_headers: dict[str, str], started: threading.Event, stop_event: threading.Event, lines_out: list[str]) -> None:
    request = urllib.request.Request(
        base + "/api/intercom/subscribe/operator.task",
        headers=auth_headers,
        method="GET",
    )
    try:
        with urllib.request.urlopen(request, timeout=20) as resp:
            started.set()
            deadline = time.time() + 20
            while time.time() < deadline and not stop_event.is_set():
                line = resp.readline()
                if not line:
                    break
                text = line.decode("utf-8", "replace").rstrip("\n")
                if text:
                    lines_out.append(text)
                    if text.startswith("data:"):
                        stop_event.set()
                        break
    except Exception as exc:
        lines_out.append(f"SSE_ERROR:{type(exc).__name__}:{exc}")
        started.set()


def run_smoke(base: str, out: str, container: str, api_key: str | None) -> dict[str, Any]:
    if api_key is None:
        api_key = container_api_key(container)
    auth_headers = {"Authorization": f"Bearer {api_key}"} if api_key else {}
    results: dict[str, Any] = {}

    for _attempt in range(1, 31):
        health = req(base, auth_headers, "GET", "/health", timeout=5)
        results["health"] = health
        if health.get("ok"):
            break
        time.sleep(2)

    results["ready"] = req(base, auth_headers, "GET", "/api/health/ready", timeout=10)
    results["agents"] = req(base, auth_headers, "GET", "/api/agents", timeout=20)

    started = threading.Event()
    stop = threading.Event()
    sse_lines: list[str] = []
    thread = threading.Thread(target=sse_reader, args=(base, auth_headers, started, stop, sse_lines), daemon=True)
    thread.start()
    started.wait(timeout=5)

    results["topics_before_operator_task"] = req(base, auth_headers, "GET", "/api/intercom/topics", timeout=10)
    body = {
        "task": "Live smoke: report current SERA deployment health with one helper. Return one concise sentence.",
        "agent": "sera",
        "helper": "operator-helper",
    }
    results["operator_task"] = req(base, auth_headers, "POST", "/api/operator/tasks", body=body, timeout=180)
    card = results["operator_task"].get("json") or {}
    session_key = card.get("session_key") if isinstance(card, dict) else None
    if session_key:
        results["subagents"] = req(base, auth_headers, "GET", f"/api/sessions/{session_key}/subagents", timeout=20)
    else:
        results["subagents"] = {"ok": False, "status": None, "raw": "no session_key returned", "json": None}

    time.sleep(2)
    stop.set()
    thread.join(timeout=2)
    results["sse_lines"] = sse_lines[:20]
    results["topics_after_operator_task"] = req(base, auth_headers, "GET", "/api/intercom/topics", timeout=10)

    with open(out, "w", encoding="utf-8") as handle:
        json.dump(results, handle, indent=2)
    return results


def ready_runtime_connected(ready_json: Any) -> bool:
    return isinstance(ready_json, dict) and ready_json.get("runtime_connected") is True


def build_summary(results: dict[str, Any], out: str) -> dict[str, Any]:
    card = (results.get("operator_task") or {}).get("json") or {}
    ready_json = (results.get("ready") or {}).get("json")
    agents_json = (results.get("agents") or {}).get("json")
    subagents_json = (results.get("subagents") or {}).get("json") or {}
    sse_lines = results.get("sse_lines") or []
    agent_count = len(agents_json) if isinstance(agents_json, list) else None
    subagent_count = len(subagents_json.get("subagents", [])) if isinstance(subagents_json, dict) else None

    return {
        "health_status": (results.get("health") or {}).get("status"),
        "ready_status": (results.get("ready") or {}).get("status"),
        "runtime_connected": ready_json.get("runtime_connected") if isinstance(ready_json, dict) else None,
        "agents_status": (results.get("agents") or {}).get("status"),
        "agent_count": agent_count,
        "operator_status": (results.get("operator_task") or {}).get("status"),
        "operator_card_status": card.get("status") if isinstance(card, dict) else None,
        "operator_blocked": card.get("blocked") if isinstance(card, dict) else None,
        "failure_class": card.get("failure_class") if isinstance(card, dict) else None,
        "next_action": card.get("next_action") if isinstance(card, dict) else None,
        "interrupted_result": is_interrupted_runtime_result(card.get("result") if isinstance(card, dict) else None),
        "session_key_present": bool(card.get("session_key")) if isinstance(card, dict) else False,
        "handoff_tool": card.get("handoff_tool") if isinstance(card, dict) else None,
        "helper_agent": (card.get("spawned_helper") or {}).get("agent") if isinstance(card.get("spawned_helper") if isinstance(card, dict) else None, dict) else None,
        "subagents_status": (results.get("subagents") or {}).get("status"),
        "subagent_count": subagent_count,
        "sse_data_seen": any(isinstance(line, str) and line.startswith("data:") and "operator.task" in line for line in sse_lines),
        "topics_before": (results.get("topics_before_operator_task") or {}).get("json"),
        "full_output": out,
    }


def validation_errors(results: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    card = (results.get("operator_task") or {}).get("json") or {}
    ready = results.get("ready") or {}
    ready_json = ready.get("json")
    subagents_json = (results.get("subagents") or {}).get("json") or {}
    subagent_count = len(subagents_json.get("subagents", [])) if isinstance(subagents_json, dict) else 0
    session_key = card.get("session_key") if isinstance(card, dict) else None
    result = card.get("result") if isinstance(card, dict) else None
    sse_lines = results.get("sse_lines") or []
    sse_data_seen = any(isinstance(line, str) and line.startswith("data:") and "operator.task" in line for line in sse_lines)

    checks = [
        ((results.get("health") or {}).get("status") == 200, "health status must be 200"),
        (ready.get("status") == 200, "readiness status must be 200"),
        (ready_runtime_connected(ready_json), "readiness must report runtime_connected=true"),
        ((results.get("agents") or {}).get("status") == 200, "agents status must be 200"),
        ((results.get("operator_task") or {}).get("status") == 200, "operator task POST must return 200"),
        (bool(session_key), "operator task card must include session_key"),
        ((results.get("subagents") or {}).get("status") == 200, "subagents status must be 200"),
        (subagent_count >= 1, "subagents response must include at least one subagent"),
        (sse_data_seen, "SSE subscription must receive an operator.task data event"),
        (not is_interrupted_runtime_result(result), "operator task result must not be interrupted/doom-loop"),
    ]
    for ok, message in checks:
        if not ok:
            errors.append(message)

    if isinstance(card, dict) and is_interrupted_runtime_result(result):
        if card.get("status") == "complete" or card.get("blocked") is False:
            errors.append("interrupted operator task must not be status=complete or blocked=false")
        if not card.get("failure_class"):
            errors.append("interrupted operator task should expose failure_class")
        if not card.get("next_action"):
            errors.append("interrupted operator task should expose next_action")
    return errors


def load_results(path: str) -> dict[str, Any]:
    with open(path, "r", encoding="utf-8") as handle:
        data = json.load(handle)
    if not isinstance(data, dict):
        raise ValueError(f"{path} does not contain a JSON object")
    return data


def main() -> int:
    parser = argparse.ArgumentParser(description="Run or validate strict SERA operator-task smoke checks.")
    parser.add_argument("--base", default=DEFAULT_BASE)
    parser.add_argument("--out", default=DEFAULT_OUT)
    parser.add_argument("--container", default=DEFAULT_CONTAINER)
    parser.add_argument("--api-key", default=None, help="Bearer token; defaults to SERA_API_KEY from docker inspect.")
    parser.add_argument("--validate-only", metavar="PATH", help="Validate an existing smoke JSON artifact without live side effects.")
    args = parser.parse_args()

    if args.validate_only:
        results = load_results(args.validate_only)
        out = args.validate_only
    else:
        results = run_smoke(args.base.rstrip("/"), args.out, args.container, args.api_key)
        out = args.out

    summary = build_summary(results, out)
    errors = validation_errors(results)
    if errors:
        summary["validation_errors"] = errors
    print(json.dumps(summary, indent=2))
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
