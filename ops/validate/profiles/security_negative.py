"""sera-validate security_negative profile (sera-sv76.2).

Runs P0 negative cases against a live SERA gateway:

  - SEC-AUTH-01..06   live-safe auth-negative cases (no model required).
  - SEC-FALSE-01..02  validate-only false-green checks against bundled fixtures.

Cases that require a model or an ephemeral compose overlay (SSRF/CAP/ERR) are
recorded as explicit skipped specs (`runtime_required` or `ephemeral_only`) so
they surface as not-yet-implemented instead of silently passing.

Stdlib only. The profile module is self-contained; it loads
`ops/e2e/operator_task_smoke.py` via importlib for false-green semantics so the
strict checks live in exactly one place (same trick sv76.1 uses).
"""

from __future__ import annotations

import datetime as _dt
import importlib.util
import json
import re
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

_THIS = Path(__file__).resolve()
_OPS_VALIDATE = _THIS.parents[1]
_OPS_DIR = _OPS_VALIDATE.parent
_OPERATOR_SMOKE_PATH = _OPS_DIR / "e2e" / "operator_task_smoke.py"
_FIXTURES_DIR = _OPS_VALIDATE / "fixtures"

DEFAULT_HTTP_TIMEOUT = 5.0

_BEARER_TOKEN_RE = re.compile(r"Bearer\s+([A-Za-z0-9._\-]{8,})", re.IGNORECASE)


def _load_operator_smoke() -> Any:
    if not _OPERATOR_SMOKE_PATH.exists():
        raise RuntimeError(f"operator_task_smoke.py not found at {_OPERATOR_SMOKE_PATH}")
    spec = importlib.util.spec_from_file_location(
        "operator_task_smoke_security_negative", _OPERATOR_SMOKE_PATH
    )
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load spec for {_OPERATOR_SMOKE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _now_iso() -> str:
    return _dt.datetime.now(_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def _hdrs_to_dict(headers: Any) -> dict[str, str]:
    out: dict[str, str] = {}
    if headers is None:
        return out
    try:
        items = list(headers.items())
    except AttributeError:
        try:
            items = list(headers)
        except TypeError:
            return out
    for k, v in items:
        out[str(k)] = str(v)
    return out


def _http_send(
    method: str,
    url: str,
    headers: dict[str, str] | None,
    body: bytes | None,
    timeout: float = DEFAULT_HTTP_TIMEOUT,
) -> tuple[int | None, dict[str, str], bytes, str | None]:
    """Issue an HTTP request without redirect-following.

    Returns (status, response_headers, response_body, transport_error). When the
    transport itself fails (DNS, connection refused, timeout) status is None and
    the error message is reported via transport_error.
    """
    try:
        req = urllib.request.Request(url, method=method, data=body)
    except ValueError as exc:
        return None, {}, b"", f"invalid_url: {exc}"
    for k, v in (headers or {}).items():
        req.add_header(k, v)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, _hdrs_to_dict(resp.headers), resp.read(), None
    except urllib.error.HTTPError as exc:
        body_bytes: bytes
        try:
            body_bytes = exc.read() or b""
        except Exception:  # noqa: BLE001
            body_bytes = b""
        return exc.code, _hdrs_to_dict(exc.headers), body_bytes, None
    except urllib.error.URLError as exc:
        return None, {}, b"", f"url_error: {exc.reason}"
    except TimeoutError as exc:
        return None, {}, b"", f"timeout: {exc}"
    except Exception as exc:  # noqa: BLE001
        return None, {}, b"", f"{type(exc).__name__}: {exc}"


def _redact_response_body(body_bytes: bytes, known_secrets: set[str]) -> str:
    """Decode + redact for storage in artifact. Best-effort utf-8 decode."""
    try:
        text = body_bytes.decode("utf-8")
    except UnicodeDecodeError:
        text = body_bytes.decode("utf-8", "replace")
    for s in known_secrets:
        if s and s in text:
            text = text.replace(s, "<redacted:known-secret>")
    text = _BEARER_TOKEN_RE.sub("Bearer <redacted:bearer>", text)
    return text


def _detect_leaks(
    haystacks: list[str],
    known_secrets: set[str],
    forbidden_substrings: list[str] | None = None,
) -> list[str]:
    """Per-case leak detector. Returns a list of leak findings (empty = clean)."""
    leaks: list[str] = []
    for hay in haystacks:
        if not hay:
            continue
        for s in known_secrets:
            if s and s in hay:
                leaks.append(f"known_secret_present (len={len(s)})")
        for m in _BEARER_TOKEN_RE.finditer(hay):
            tok = m.group(1)
            if tok and not tok.startswith("<redacted"):
                leaks.append(f"bearer_token_shape_present (token_prefix={tok[:4]})")
        for needle in forbidden_substrings or []:
            if needle and needle in hay:
                leaks.append(f"forbidden_substring_present: {needle!r}")
    return leaks


# --- case helpers ------------------------------------------------------------


def _new_case(case_id: str, category: str, **kwargs: Any) -> dict[str, Any]:
    case: dict[str, Any] = {
        "case_id": case_id,
        "category": category,
        "live_safe": True,
        "ephemeral_only": False,
        "runtime_required": False,
        "skipped": False,
        "skip_reason": None,
        "attack": {
            "method": None,
            "path": None,
            "headers": {},
            "body": None,
            "tool_call": None,
            "url": None,
        },
        "expected": {
            "status": None,
            "blocked": None,
            "failure_class": None,
            "marker": None,
        },
        "actual": {
            "status": None,
            "body_redacted": None,
            "card": None,
            "audit": None,
            "transport_error": None,
        },
        "evidence": {
            "request_id": None,
            "audit_id": None,
            "container_log_window": None,
        },
        "audit_gap": False,
        "audit_required": False,
        "secrets_detected": False,
        "passed": None,
        "fail_reason": None,
        "started_at": None,
        "ended_at": None,
    }
    case.update(kwargs)
    return case


def _auth_case(
    *,
    case_id: str,
    base: str,
    method: str,
    path: str,
    headers: dict[str, str] | None,
    body: bytes | None,
    expected_status: int,
    forbidden_in_body: list[str],
    known_secrets: set[str],
    category: str = "auth",
) -> dict[str, Any]:
    case = _new_case(case_id, category)
    case["attack"]["method"] = method
    case["attack"]["path"] = path
    case["attack"]["headers"] = dict(headers or {})
    case["attack"]["body"] = (
        body.decode("utf-8", "replace") if isinstance(body, (bytes, bytearray)) else None
    )
    case["expected"]["status"] = expected_status
    case["expected"]["marker"] = (
        "no token reflection in response body or headers"
    )
    case["started_at"] = _now_iso()

    url = base.rstrip("/") + path
    status, resp_headers, resp_body, transport_error = _http_send(
        method, url, headers, body
    )
    case["ended_at"] = _now_iso()

    body_redacted = _redact_response_body(resp_body, known_secrets)
    headers_redacted = {
        k: ("<redacted:header>" if k.lower() in {"set-cookie", "authorization"} else v)
        for k, v in resp_headers.items()
    }
    case["actual"]["status"] = status
    case["actual"]["body_redacted"] = body_redacted
    case["actual"]["headers_redacted"] = headers_redacted
    case["actual"]["transport_error"] = transport_error

    fail_reasons: list[str] = []
    if transport_error is not None:
        fail_reasons.append(f"transport_error: {transport_error}")
    elif status != expected_status:
        fail_reasons.append(f"status {status} != expected {expected_status}")

    leak_haystacks = [body_redacted] + [str(v) for v in headers_redacted.values()]
    leaks = _detect_leaks(
        leak_haystacks, known_secrets, forbidden_substrings=forbidden_in_body
    )
    if leaks:
        case["secrets_detected"] = True
        fail_reasons.extend(leaks)

    case["passed"] = not fail_reasons
    case["fail_reason"] = "; ".join(fail_reasons) if fail_reasons else None
    return case


def _public_health_case(
    *,
    case_id: str,
    base: str,
    known_secrets: set[str],
) -> dict[str, Any]:
    """SEC-AUTH-06: /health and /api/health/ready must be reachable without auth."""
    case = _new_case(case_id, "auth-public")
    case["attack"]["method"] = "GET"
    case["attack"]["path"] = "/health,/api/health/ready"
    case["expected"]["marker"] = (
        "/health 200; /api/health/ready 200 or 503 with not_ready; no auth required"
    )
    case["started_at"] = _now_iso()

    fail_reasons: list[str] = []
    probes: dict[str, dict[str, Any]] = {}
    for path, allowed in (
        ("/health", {200}),
        ("/api/health/ready", {200, 503}),
    ):
        url = base.rstrip("/") + path
        status, resp_headers, resp_body, transport_error = _http_send(
            "GET", url, headers=None, body=None
        )
        body_redacted = _redact_response_body(resp_body, known_secrets)
        probes[path] = {
            "status": status,
            "body_redacted": body_redacted,
            "transport_error": transport_error,
        }
        if transport_error is not None:
            fail_reasons.append(f"{path}: transport_error: {transport_error}")
            continue
        if status not in allowed:
            fail_reasons.append(f"{path}: status {status} not in {sorted(allowed)}")
            continue
        if path == "/api/health/ready":
            try:
                parsed = json.loads(body_redacted) if body_redacted else None
            except (json.JSONDecodeError, ValueError):
                parsed = None
            if not isinstance(parsed, dict) or "runtime_connected" not in parsed:
                fail_reasons.append(
                    "/api/health/ready: body must include 'runtime_connected' field"
                )

    case["ended_at"] = _now_iso()
    case["actual"]["probes"] = probes

    leak_haystacks = [str(p.get("body_redacted") or "") for p in probes.values()]
    leaks = _detect_leaks(leak_haystacks, known_secrets)
    if leaks:
        case["secrets_detected"] = True
        fail_reasons.extend(leaks)

    case["passed"] = not fail_reasons
    case["fail_reason"] = "; ".join(fail_reasons) if fail_reasons else None
    return case


def _validate_only_case(
    *,
    case_id: str,
    fixture_name: str,
    expected_marker: str,
    smoke: Any,
) -> dict[str, Any]:
    case = _new_case(case_id, "false-green")
    case["attack"]["path"] = f"fixtures/{fixture_name}"
    case["expected"]["marker"] = expected_marker
    case["started_at"] = _now_iso()

    path = _FIXTURES_DIR / fixture_name
    fail_reasons: list[str] = []
    raised_errors: list[str] = []
    if not path.exists():
        fail_reasons.append(f"fixture missing: {path}")
    else:
        try:
            with open(path, "r", encoding="utf-8") as handle:
                data = json.load(handle)
        except (OSError, json.JSONDecodeError) as exc:
            fail_reasons.append(f"fixture unreadable: {exc}")
            data = None
        if isinstance(data, dict):
            evidence = (
                data["evidence"]
                if isinstance(data.get("evidence"), dict)
                else data
            )
            try:
                raised_errors = list(smoke.validation_errors(evidence))
            except Exception as exc:  # noqa: BLE001
                fail_reasons.append(
                    f"validation_errors raised: {type(exc).__name__}: {exc}"
                )
            if expected_marker and not any(
                expected_marker in e for e in raised_errors
            ):
                fail_reasons.append(
                    f"expected validation marker not raised: {expected_marker!r}; "
                    f"got: {raised_errors!r}"
                )
        elif data is not None:
            fail_reasons.append("fixture is not a JSON object")

    case["ended_at"] = _now_iso()
    case["actual"]["validation_errors"] = raised_errors
    case["passed"] = not fail_reasons
    case["fail_reason"] = "; ".join(fail_reasons) if fail_reasons else None
    return case


def _skipped_case(
    *,
    case_id: str,
    category: str,
    skip_reason: str,
    marker: str,
    runtime_required: bool = False,
    ephemeral_only: bool = False,
    audit_required: bool = False,
) -> dict[str, Any]:
    case = _new_case(
        case_id,
        category,
        live_safe=not (runtime_required or ephemeral_only),
        runtime_required=runtime_required,
        ephemeral_only=ephemeral_only,
        skipped=True,
        skip_reason=skip_reason,
        audit_required=audit_required,
        audit_gap=audit_required,
    )
    case["expected"]["marker"] = marker
    case["passed"] = None
    case["fail_reason"] = None
    return case


# --- profile entry point -----------------------------------------------------


def run(
    *,
    base: str,
    container: str,
    api_key: str,
    known_secrets: set[str] | None = None,
) -> tuple[
    dict[str, Any], dict[str, Any], list[str], list[str], list[dict[str, Any]]
]:
    """Run the security_negative profile.

    Returns ``(summary, evidence, errors, warnings, negative_cases)`` matching
    the dispatch contract used by ``sera-validate.py`` ``main()``.
    """
    secrets: set[str] = set(known_secrets or set())
    if api_key:
        secrets.add(api_key)
    secrets.add("sera_bootstrap_dev_123")

    cases: list[dict[str, Any]] = []

    # SEC-AUTH-01: GET /api/agents, no Authorization header.
    cases.append(
        _auth_case(
            case_id="SEC-AUTH-01",
            base=base,
            method="GET",
            path="/api/agents",
            headers=None,
            body=None,
            expected_status=401,
            forbidden_in_body=[],
            known_secrets=secrets,
        )
    )
    # SEC-AUTH-02: GET /api/agents with bogus bearer; body must not echo it.
    bogus = "xxx-not-a-key"
    cases.append(
        _auth_case(
            case_id="SEC-AUTH-02",
            base=base,
            method="GET",
            path="/api/agents",
            headers={"Authorization": f"Bearer {bogus}"},
            body=None,
            expected_status=401,
            forbidden_in_body=[bogus],
            known_secrets=secrets,
        )
    )
    # SEC-AUTH-03: POST /api/operator/tasks with no auth.
    cases.append(
        _auth_case(
            case_id="SEC-AUTH-03",
            base=base,
            method="POST",
            path="/api/operator/tasks",
            headers={"Content-Type": "application/json"},
            body=b'{"task":"diag"}',
            expected_status=401,
            forbidden_in_body=[],
            known_secrets=secrets,
        )
    )
    # SEC-AUTH-04: GET /api/agents with garbage header (no Bearer prefix).
    garbage = "garbage no-bearer-prefix"
    cases.append(
        _auth_case(
            case_id="SEC-AUTH-04",
            base=base,
            method="GET",
            path="/api/agents",
            headers={"Authorization": garbage},
            body=None,
            expected_status=401,
            forbidden_in_body=[garbage],
            known_secrets=secrets,
        )
    )
    # SEC-AUTH-05: GET /api/agents with empty bearer.
    cases.append(
        _auth_case(
            case_id="SEC-AUTH-05",
            base=base,
            method="GET",
            path="/api/agents",
            headers={"Authorization": "Bearer "},
            body=None,
            expected_status=401,
            forbidden_in_body=[],
            known_secrets=secrets,
        )
    )
    # SEC-AUTH-06: public health endpoints.
    cases.append(
        _public_health_case(
            case_id="SEC-AUTH-06",
            base=base,
            known_secrets=secrets,
        )
    )

    # SEC-FALSE-01..02: validate-only against bundled false-green fixtures.
    smoke = _load_operator_smoke()
    cases.append(
        _validate_only_case(
            case_id="SEC-FALSE-01",
            fixture_name="false_green_interrupted.json",
            expected_marker=(
                "interrupted operator task must not be status=complete or blocked=false"
            ),
            smoke=smoke,
        )
    )
    cases.append(
        _validate_only_case(
            case_id="SEC-FALSE-02",
            fixture_name="false_green_llm_error_unblocked.json",
            expected_marker="LLM/provider errors must be status=blocked",
            smoke=smoke,
        )
    )

    # Skipped specs for cases that need a model dispatch or an ephemeral overlay.
    cases.append(
        _skipped_case(
            case_id="SEC-SSRF-01",
            category="ssrf",
            skip_reason="runtime_required",
            marker="operator task must dispatch http_request to forbidden URL",
            runtime_required=True,
        )
    )
    cases.append(
        _skipped_case(
            case_id="SEC-SSRF-02",
            category="ssrf",
            skip_reason="runtime_required",
            marker="cloud-metadata IP (169.254.169.254) must be blocked",
            runtime_required=True,
        )
    )
    cases.append(
        _skipped_case(
            case_id="SEC-SSRF-03",
            category="ssrf",
            skip_reason="runtime_required",
            marker="RFC1918 (10.0.0.1) must be blocked",
            runtime_required=True,
        )
    )
    cases.append(
        _skipped_case(
            case_id="SEC-SSRF-04",
            category="ssrf",
            skip_reason="runtime_required",
            marker="IPv6 loopback ([::1]) must be blocked",
            runtime_required=True,
        )
    )
    cases.append(
        _skipped_case(
            case_id="SEC-CAP-01",
            category="capability",
            skip_reason="runtime_required",
            marker=(
                "CapabilityRegistry::check denies tier-1 agent calling tier-2 tool"
            ),
            runtime_required=True,
            audit_required=True,
        )
    )
    cases.append(
        _skipped_case(
            case_id="SEC-CAP-02",
            category="capability-live",
            skip_reason="runtime_required",
            marker="operator task with disallowed tool blocks; no tool result echo",
            runtime_required=True,
            audit_required=True,
        )
    )
    cases.append(
        _skipped_case(
            case_id="SEC-CAP-03",
            category="capability-missing",
            skip_reason="ephemeral_only",
            marker="gateway boot fails closed when policy_ref is missing",
            ephemeral_only=True,
        )
    )
    cases.append(
        _skipped_case(
            case_id="SEC-ERR-01",
            category="provider-error",
            skip_reason="runtime_required",
            marker="card.blocked=true; reply has no provider URL or *_KEY shape",
            runtime_required=True,
        )
    )
    cases.append(
        _skipped_case(
            case_id="SEC-ERR-02",
            category="empty-assistant",
            skip_reason="ephemeral_only",
            marker=(
                "[LLM unavailable: model returned an empty response; retry the turn.]"
            ),
            ephemeral_only=True,
        )
    )
    cases.append(
        _skipped_case(
            case_id="SEC-ERR-03",
            category="raw-provider-error",
            skip_reason="ephemeral_only",
            marker="reply must not contain provider host or provider credential shape",
            ephemeral_only=True,
        )
    )

    # Aggregate
    counts = summarize_cases(cases)
    summary: dict[str, Any] = {"security_negative": counts}

    errors: list[str] = []
    warnings: list[str] = []
    for case in cases:
        if case.get("skipped"):
            continue
        if case.get("secrets_detected"):
            errors.append(f"secrets_leaked: {case['case_id']}")
        if case.get("passed") is False:
            errors.append(
                f"{case['case_id']}: {case.get('fail_reason') or 'failed'}"
            )
        if case.get("audit_required") and case.get("audit_gap"):
            warnings.append(f"audit_gap: {case['case_id']}")

    evidence: dict[str, Any] = {
        "base": base,
        "container": container,
        "case_summary": counts,
    }
    return summary, evidence, errors, warnings, cases


def summarize_cases(cases: list[dict[str, Any]]) -> dict[str, int]:
    """Compute summary counters expected by ``summary.security_negative``."""
    counts = {
        "cases_total": len(cases),
        "cases_passed": 0,
        "cases_failed": 0,
        "audit_gaps": 0,
        "live_only_skipped": 0,
        "ephemeral_only_skipped": 0,
    }
    for case in cases:
        if case.get("skipped"):
            if case.get("ephemeral_only"):
                counts["ephemeral_only_skipped"] += 1
            elif case.get("runtime_required"):
                counts["live_only_skipped"] += 1
            continue
        if case.get("passed") is True:
            counts["cases_passed"] += 1
        elif case.get("passed") is False:
            counts["cases_failed"] += 1
        if case.get("audit_required") and case.get("audit_gap"):
            counts["audit_gaps"] += 1
    return counts
