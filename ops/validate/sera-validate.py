#!/usr/bin/env python3
"""sera-validate — Docker-native validation runner skeleton (sera-sv76.1).

Profiles:
  - live_smoke      wraps ops/e2e/operator_task_smoke.py via importlib so the
                    strict false-green semantics live in exactly one place.

Modes:
  --profile <name>          run a profile, write artifact, validate, exit per rubric.
  --validate-only <path>    re-validate a captured artifact without live side effects.

Exit-code rubric:
  0  pass            all errors empty.
  1  warn            non-empty warnings, empty errors.
  2  fail            any strict check failed (false-green, secret leak, runtime down).
  3  harness error   docker missing, file missing, artifact malformed, etc.

Stdlib only. No pip dependencies.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import importlib.util
import json
import os
import re
import subprocess
import sys
import tempfile
import uuid
from pathlib import Path
from typing import Any, Callable

# --- constants ---------------------------------------------------------------

DEFAULT_BASE = "http://127.0.0.1:3001"
DEFAULT_CONTAINER = "rust-sera-1"
SCHEMA_VERSION = "1.1.0"
TOOL_VERSION = "0.2.0"
REDACTION_RULES_VERSION = "1"

EXIT_PASS = 0
EXIT_WARN = 1
EXIT_FAIL = 2
EXIT_HARNESS_ERROR = 3

VERDICT_BY_EXIT = {
    EXIT_PASS: "pass",
    EXIT_WARN: "warn",
    EXIT_FAIL: "fail",
    EXIT_HARNESS_ERROR: "harness_error",
}

# --- redaction ---------------------------------------------------------------

REDACT_HEADER_KEYS = {
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "x-sera-token",
    "x-admin-token",
}

REDACT_FIELD_NAMES = {
    "token",
    "api_key",
    "secret",
    "client_secret",
    "password",
    "sera_api_key",
    "sera_admin_token",
}

REDACTED_HEADER = "<redacted:header>"
REDACTED_BEARER = "Bearer <redacted:bearer>"
REDACTED_FIELD = "<redacted:field>"
REDACTED_ENV = "<redacted:env>"

_BEARER_RE = re.compile(r"Bearer\s+([A-Za-z0-9._\-]{8,})", re.IGNORECASE)
_ENV_RE = re.compile(
    r"(SERA_API_KEY|SERA_ADMIN_TOKEN|[A-Z][A-Z0-9_]*_(?:API_KEY|TOKEN|SECRET))=([^\s\"',]+)"
)

REDACTION_PATTERNS = [
    "header:Authorization/Cookie/X-Api-Key/...",
    "field:token/api_key/secret/client_secret/password",
    "bearer <opaque>",
    "ENV_KEY=<opaque>",
    "literal known secrets (api_key, dev key)",
]


class _Counter:
    __slots__ = ("n",)

    def __init__(self) -> None:
        self.n = 0

    def bump(self, by: int = 1) -> None:
        self.n += by


def _redact_string(value: str, counter: _Counter) -> str:
    def _bearer_sub(_m: "re.Match[str]") -> str:
        counter.bump()
        return REDACTED_BEARER

    def _env_sub(m: "re.Match[str]") -> str:
        counter.bump()
        return f"{m.group(1)}={REDACTED_ENV}"

    out = _BEARER_RE.sub(_bearer_sub, value)
    out = _ENV_RE.sub(_env_sub, out)
    return out


def _redact_value(value: Any, counter: _Counter) -> Any:
    if isinstance(value, dict):
        new: dict[str, Any] = {}
        for k, v in value.items():
            kl = k.lower() if isinstance(k, str) else None
            if kl in REDACT_HEADER_KEYS:
                if v != REDACTED_HEADER:
                    counter.bump()
                new[k] = REDACTED_HEADER
            elif kl in REDACT_FIELD_NAMES:
                if v != REDACTED_FIELD:
                    counter.bump()
                new[k] = REDACTED_FIELD
            else:
                new[k] = _redact_value(v, counter)
        return new
    if isinstance(value, list):
        return [_redact_value(item, counter) for item in value]
    if isinstance(value, str):
        return _redact_string(value, counter)
    return value


def redact_artifact(artifact: dict[str, Any], known_secrets: set[str] | None = None) -> tuple[dict[str, Any], int]:
    """Return a redacted copy of artifact and the count of redactions applied."""
    counter = _Counter()
    redacted = _redact_value(artifact, counter)
    if known_secrets:
        serialized = json.dumps(redacted)
        for secret in known_secrets:
            if not secret:
                continue
            if secret in serialized:
                serialized = serialized.replace(secret, REDACTED_FIELD)
                counter.bump()
        redacted = json.loads(serialized)
    return redacted, counter.n


def redaction_self_check(artifact: dict[str, Any], known_secrets: set[str] | None) -> list[str]:
    """Scan a redacted artifact for literal secret survival. Return list of leak findings."""
    leaks: list[str] = []
    serialized = json.dumps(artifact)
    if known_secrets:
        for secret in known_secrets:
            if secret and secret in serialized:
                leaks.append(
                    f"secret survived redaction (post-serialize self-check failed): "
                    f"{secret[:4]}…(len={len(secret)})"
                )
    # Also flag any bearer-like or env-style strings that escaped.
    if _BEARER_RE.search(serialized):
        # Allow the placeholder we wrote; only flag genuine token shapes.
        for m in _BEARER_RE.finditer(serialized):
            tok = m.group(1)
            if tok != "<redacted:bearer>" and not tok.startswith("<redacted"):
                leaks.append("bearer token shape survived redaction")
                break
    return leaks


# --- harness errors ----------------------------------------------------------


class HarnessError(Exception):
    """Raised when the runner cannot fulfill its prerequisites."""


# --- docker discovery --------------------------------------------------------


def container_api_key(container: str) -> str:
    """Return SERA_API_KEY from `docker inspect <container>`. Raises HarnessError on failure."""
    if not container:
        raise HarnessError("container name is required to discover SERA_API_KEY")
    try:
        raw = subprocess.check_output(
            ["docker", "inspect", container],
            text=True,
            stderr=subprocess.PIPE,
        )
    except FileNotFoundError as exc:
        raise HarnessError(f"docker binary not found on PATH: {exc}") from exc
    except subprocess.CalledProcessError as exc:
        msg = (exc.stderr or "").strip() or str(exc)
        raise HarnessError(f"docker inspect {container} failed: {msg}") from exc
    try:
        data = json.loads(raw)[0]
    except (json.JSONDecodeError, IndexError) as exc:
        raise HarnessError(f"docker inspect output unparseable for {container}: {exc}") from exc
    for env in data.get("Config", {}).get("Env", []) or []:
        if isinstance(env, str) and env.startswith("SERA_API_KEY="):
            return env.split("=", 1)[1]
    return ""


def discover_api_key(explicit: str | None, container: str) -> str:
    """Resolve SERA_API_KEY: explicit flag > env > docker inspect."""
    if explicit:
        return explicit
    env_val = os.environ.get("SERA_API_KEY")
    if env_val:
        return env_val
    return container_api_key(container)


# --- operator_task_smoke loader ---------------------------------------------


_OPERATOR_SMOKE_PATH = (
    Path(__file__).resolve().parents[1] / "e2e" / "operator_task_smoke.py"
)


def _load_operator_smoke() -> Any:
    if not _OPERATOR_SMOKE_PATH.exists():
        raise HarnessError(
            f"operator_task_smoke.py not found at {_OPERATOR_SMOKE_PATH}"
        )
    spec = importlib.util.spec_from_file_location(
        "operator_task_smoke", _OPERATOR_SMOKE_PATH
    )
    if spec is None or spec.loader is None:
        raise HarnessError(f"failed to load spec for {_OPERATOR_SMOKE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# --- artifact construction ---------------------------------------------------


def _now_iso() -> str:
    return _dt.datetime.now(_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def _run_id(profile: str) -> str:
    stamp = _dt.datetime.now(_dt.timezone.utc).strftime("%Y-%m-%dT%H-%M-%SZ")
    return f"{stamp}-{uuid.uuid4().hex[:6]}-{profile}"


def build_artifact(
    profile: str,
    run_id: str,
    started_at: str,
    ended_at: str,
    target: dict[str, Any],
    summary: dict[str, Any],
    evidence: dict[str, Any],
    errors: list[str],
    warnings: list[str],
    negative_cases: list[dict[str, Any]],
) -> dict[str, Any]:
    return {
        "schema_version": SCHEMA_VERSION,
        "profile": profile,
        "run_id": run_id,
        "started_at": started_at,
        "ended_at": ended_at,
        "target": target,
        "harness": {
            "tool": "sera-validate",
            "version": TOOL_VERSION,
            "python": (
                f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}"
            ),
            "host": sys.platform,
        },
        "redaction": {
            "applied": True,
            "rules_version": REDACTION_RULES_VERSION,
            "redacted_count": 0,
            "patterns": list(REDACTION_PATTERNS),
        },
        "summary": summary,
        "validation": {
            "errors": list(errors),
            "warnings": list(warnings),
            "negative_cases": list(negative_cases),
            "verdict": verdict_for(errors, warnings),
        },
        "evidence": evidence,
        "exit_code": exit_code_for(errors, warnings),
    }


def verdict_for(errors: list[str], warnings: list[str]) -> str:
    return VERDICT_BY_EXIT[exit_code_for(errors, warnings)]


def exit_code_for(errors: list[str], warnings: list[str]) -> int:
    if errors:
        return EXIT_FAIL
    if warnings:
        return EXIT_WARN
    return EXIT_PASS


# --- profile: live_smoke -----------------------------------------------------


def profile_live_smoke(
    *,
    base: str,
    container: str,
    api_key: str,
) -> tuple[dict[str, Any], dict[str, Any], list[str], list[str]]:
    """Run live_smoke. Returns (summary, evidence, errors, warnings)."""
    smoke = _load_operator_smoke()
    with tempfile.NamedTemporaryFile(
        prefix="sera-validate-livesmoke-", suffix=".json", delete=False
    ) as tmp:
        tmp_path = tmp.name
    try:
        results = smoke.run_smoke(base, tmp_path, container, api_key)
        summary = smoke.build_summary(results, tmp_path)
        errors = smoke.validation_errors(results)
        # Wrap the existing harness summary in our normalized layout.
        summary["redaction_ok"] = True  # filled in after self-check
        warnings: list[str] = []
        return summary, results, errors, warnings
    finally:
        try:
            os.unlink(tmp_path)
        except OSError:
            pass


# --- validate-only -----------------------------------------------------------


def _coerce_evidence(loaded: dict[str, Any]) -> dict[str, Any]:
    """Accept either the new wrapped artifact (with .evidence) or the old flat results dict."""
    if isinstance(loaded.get("evidence"), dict):
        return loaded["evidence"]
    return loaded


def validate_only(path: str) -> tuple[int, dict[str, Any]]:
    """Re-validate a captured artifact. Returns (exit_code, report dict)."""
    p = Path(path)
    if not p.exists():
        raise HarnessError(f"validate-only artifact not found: {path}")
    try:
        with open(p, "r", encoding="utf-8") as handle:
            loaded = json.load(handle)
    except (OSError, json.JSONDecodeError) as exc:
        raise HarnessError(f"validate-only artifact unreadable: {exc}") from exc
    if not isinstance(loaded, dict):
        raise HarnessError(f"validate-only artifact is not a JSON object: {path}")

    smoke = _load_operator_smoke()
    evidence = _coerce_evidence(loaded)
    errors = smoke.validation_errors(evidence)
    summary = smoke.build_summary(evidence, str(p))

    # Self-check redaction on the loaded artifact (no live secrets known here, so
    # only structural checks: bearer/env-shaped survivors).
    leaks = redaction_self_check(loaded, known_secrets=None)
    if leaks:
        errors = list(errors) + leaks

    warnings: list[str] = []
    report = {
        "profile": loaded.get("profile") or "validate_only",
        "validated_path": str(p),
        "summary": summary,
        "validation": {
            "errors": errors,
            "warnings": warnings,
            "verdict": verdict_for(errors, warnings),
        },
    }
    return exit_code_for(errors, warnings), report


# --- profile registry --------------------------------------------------------

PROFILES: dict[str, str] = {
    "live_smoke": "Strict operator-task live smoke against rust-sera-1.",
    "security_negative": (
        "P0 security-negative cases (auth, false-green, "
        "and runtime/ephemeral-skipped specs)."
    ),
    "docker_security": (
        "Docker security overlay validation with structured compose skips/findings."
    ),
    "perf_latency": (
        "Docker-native latency baseline: health/ready/operator percentiles, "
        "error rate, and container stats."
    ),
    "perf_reliability": (
        "Docker-native reliability baseline: smoke/baseline loops, runtime "
        "disconnects, restarts, zombies, hangs, and false-green detection."
    ),
}


# --- profile loader ----------------------------------------------------------


def _load_profile_module(name: str) -> Any:
    """Load a profile sub-module by name (e.g. 'security_negative')."""
    candidate = Path(__file__).resolve().parent / "profiles" / f"{name}.py"
    if not candidate.exists():
        raise HarnessError(f"profile module missing: {candidate}")
    spec = importlib.util.spec_from_file_location(
        f"sera_validate_profile_{name}", candidate
    )
    if spec is None or spec.loader is None:
        raise HarnessError(f"failed to load spec for profile module {candidate}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def profile_security_negative(
    *,
    base: str,
    container: str,
    api_key: str,
    known_secrets: set[str],
) -> tuple[
    dict[str, Any], dict[str, Any], list[str], list[str], list[dict[str, Any]]
]:
    """Dispatch wrapper around `profiles.security_negative.run`."""
    module = _load_profile_module("security_negative")
    return module.run(
        base=base,
        container=container,
        api_key=api_key,
        known_secrets=known_secrets,
    )


def profile_docker_security(
    *, run_compose: bool = False
) -> tuple[dict[str, Any], dict[str, Any], list[str], list[str], list[dict[str, Any]]]:
    """Dispatch wrapper around `profiles.docker_security.run`.

    The default CLI run is non-destructive: it validates the overlay file and
    compose rendering, but does not start containers. Passing ``--run-compose``
    explicitly exercises an ephemeral `sera-sec-<runid>` compose lifecycle and
    records startup incompatibilities as findings while still attempting
    cleanup.
    """
    module = _load_profile_module("docker_security")
    return module.run(run_compose=run_compose)


def profile_perf_latency(
    *,
    base: str,
    container: str,
    api_key: str,
    health_samples: int,
) -> tuple[dict[str, Any], dict[str, Any], list[str], list[str], list[dict[str, Any]]]:
    """Dispatch wrapper around `profiles.performance.run_latency`."""
    module = _load_profile_module("performance")
    return module.run_latency(
        base=base,
        container=container,
        api_key=api_key,
        health_samples=health_samples,
    )


def profile_perf_reliability(
    *,
    base: str,
    container: str,
    api_key: str,
    mode: str,
    duration_seconds: int | None,
    interval_seconds: int,
) -> tuple[dict[str, Any], dict[str, Any], list[str], list[str], list[dict[str, Any]]]:
    """Dispatch wrapper around `profiles.performance.run_reliability`."""
    module = _load_profile_module("performance")
    return module.run_reliability(
        base=base,
        container=container,
        api_key=api_key,
        mode=mode,
        duration_seconds=duration_seconds,
        interval_seconds=interval_seconds,
    )


# --- CLI ---------------------------------------------------------------------


def _build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="sera-validate",
        description="Docker-native validation runner for SERA (sera-sv76.1 skeleton).",
    )
    parser.add_argument("--profile", choices=sorted(PROFILES), help="profile name to run")
    parser.add_argument("--base", default=DEFAULT_BASE, help="base URL of the live SERA gateway")
    parser.add_argument("--container", default=DEFAULT_CONTAINER, help="docker container name for SERA_API_KEY discovery")
    parser.add_argument("--api-key", default=None, help="bearer token; defaults to SERA_API_KEY discovery")
    parser.add_argument("--out", default=None, help="path for the artifact JSON")
    parser.add_argument("--validate-only", metavar="PATH", help="validate an existing artifact without live side effects")
    parser.add_argument(
        "--run-compose",
        action="store_true",
        help=(
            "docker_security only: explicitly start an ephemeral security "
            "compose project and clean it up"
        ),
    )
    parser.add_argument(
        "--perf-mode",
        choices=("smoke", "baseline"),
        default="smoke",
        help="perf_reliability mode: smoke=5 minutes, baseline=30 minutes",
    )
    parser.add_argument(
        "--duration-seconds",
        type=int,
        default=None,
        help="override perf_reliability mode duration for tests or bounded runs",
    )
    parser.add_argument(
        "--interval-seconds",
        type=int,
        default=60,
        help="perf_reliability sample interval (default: 60)",
    )
    parser.add_argument(
        "--health-samples",
        type=int,
        default=10,
        help="perf_latency health/ready sample count (default: 10)",
    )
    parser.add_argument("--quiet", action="store_true", help="single-line summary on green; full JSON on non-green")
    return parser


def _print_summary(report: dict[str, Any], quiet: bool, artifact_path: str | None) -> None:
    verdict = (report.get("validation") or {}).get("verdict") or report.get("verdict") or "?"
    if quiet and verdict == "pass":
        line = f"sera-validate: pass profile={report.get('profile','?')}"
        if artifact_path:
            line += f" artifact={artifact_path}"
        print(line)
        return
    print(json.dumps(report, indent=2, sort_keys=True))
    if artifact_path:
        print(artifact_path)


def main(argv: list[str] | None = None) -> int:
    parser = _build_arg_parser()
    args = parser.parse_args(argv)

    if args.validate_only:
        try:
            code, report = validate_only(args.validate_only)
        except HarnessError as exc:
            sys.stderr.write(f"sera-validate: harness error: {exc}\n")
            return EXIT_HARNESS_ERROR
        _print_summary(report, args.quiet, args.validate_only)
        return code

    if not args.profile:
        sys.stderr.write("sera-validate: --profile or --validate-only is required\n")
        return EXIT_HARNESS_ERROR

    if args.profile not in PROFILES:
        sys.stderr.write(f"sera-validate: unknown profile {args.profile!r}\n")
        return EXIT_HARNESS_ERROR

    if args.run_compose and args.profile != "docker_security":
        sys.stderr.write("sera-validate: --run-compose is only supported with --profile docker_security\n")
        return EXIT_HARNESS_ERROR
    if args.duration_seconds is not None and args.duration_seconds < 0:
        sys.stderr.write("sera-validate: --duration-seconds must be non-negative\n")
        return EXIT_HARNESS_ERROR
    if args.interval_seconds <= 0:
        sys.stderr.write("sera-validate: --interval-seconds must be positive\n")
        return EXIT_HARNESS_ERROR
    if args.health_samples <= 0:
        sys.stderr.write("sera-validate: --health-samples must be positive\n")
        return EXIT_HARNESS_ERROR

    base = args.base.rstrip("/")
    if "://" not in base:
        sys.stderr.write(f"sera-validate: invalid --base URL: {args.base!r}\n")
        return EXIT_HARNESS_ERROR

    out_path = args.out or f"/tmp/sera-validate-{args.profile}.json"

    try:
        api_key = (
            ""
            if args.profile == "docker_security"
            else discover_api_key(args.api_key, args.container)
        )
    except HarnessError as exc:
        sys.stderr.write(f"sera-validate: harness error: {exc}\n")
        return EXIT_HARNESS_ERROR

    known_secrets: set[str] = set()
    if api_key:
        known_secrets.add(api_key)
    # Always treat the dev bootstrap key as a known secret if it appears.
    known_secrets.add("sera_bootstrap_dev_123")

    started_at = _now_iso()
    run_id = _run_id(args.profile)
    target = {
        "base": base,
        "container": args.container,
        "compose_project": "rust",
    }

    negative_cases: list[dict[str, Any]] = []
    try:
        if args.profile == "live_smoke":
            summary, evidence, errors, warnings = profile_live_smoke(
                base=base, container=args.container, api_key=api_key
            )
        elif args.profile == "security_negative":
            (
                summary,
                evidence,
                errors,
                warnings,
                negative_cases,
            ) = profile_security_negative(
                base=base,
                container=args.container,
                api_key=api_key,
                known_secrets=known_secrets,
            )
        elif args.profile == "docker_security":
            (
                summary,
                evidence,
                errors,
                warnings,
                negative_cases,
            ) = profile_docker_security(run_compose=args.run_compose)
        elif args.profile == "perf_latency":
            (
                summary,
                evidence,
                errors,
                warnings,
                negative_cases,
            ) = profile_perf_latency(
                base=base,
                container=args.container,
                api_key=api_key,
                health_samples=args.health_samples,
            )
        elif args.profile == "perf_reliability":
            (
                summary,
                evidence,
                errors,
                warnings,
                negative_cases,
            ) = profile_perf_reliability(
                base=base,
                container=args.container,
                api_key=api_key,
                mode=args.perf_mode,
                duration_seconds=args.duration_seconds,
                interval_seconds=args.interval_seconds,
            )
        else:  # pragma: no cover - guarded by PROFILES check above
            raise HarnessError(f"profile {args.profile!r} not implemented")
    except HarnessError as exc:
        sys.stderr.write(f"sera-validate: harness error: {exc}\n")
        return EXIT_HARNESS_ERROR
    except Exception as exc:  # noqa: BLE001
        sys.stderr.write(f"sera-validate: harness error: {type(exc).__name__}: {exc}\n")
        return EXIT_HARNESS_ERROR

    ended_at = _now_iso()
    artifact = build_artifact(
        profile=args.profile,
        run_id=run_id,
        started_at=started_at,
        ended_at=ended_at,
        target=target,
        summary=summary,
        evidence=evidence,
        errors=errors,
        warnings=warnings,
        negative_cases=negative_cases,
    )

    redacted, count = redact_artifact(artifact, known_secrets=known_secrets)
    redacted["redaction"]["redacted_count"] = count

    leaks = redaction_self_check(redacted, known_secrets=known_secrets)
    if leaks:
        redacted["redaction"]["applied"] = False
        new_errors = list(redacted["validation"]["errors"]) + leaks
        redacted["validation"]["errors"] = new_errors
        redacted["validation"]["verdict"] = verdict_for(new_errors, redacted["validation"]["warnings"])
        redacted["exit_code"] = exit_code_for(new_errors, redacted["validation"]["warnings"])

    if isinstance(redacted.get("summary"), dict):
        redacted["summary"]["redaction_ok"] = bool(redacted["redaction"]["applied"])

    out_dir = Path(out_path).parent
    if str(out_dir) and not out_dir.exists():
        try:
            out_dir.mkdir(parents=True, exist_ok=True)
        except OSError as exc:
            sys.stderr.write(f"sera-validate: harness error: cannot create {out_dir}: {exc}\n")
            return EXIT_HARNESS_ERROR

    try:
        with open(out_path, "w", encoding="utf-8") as handle:
            json.dump(redacted, handle, indent=2, sort_keys=True)
    except OSError as exc:
        sys.stderr.write(f"sera-validate: harness error: cannot write {out_path}: {exc}\n")
        return EXIT_HARNESS_ERROR

    code = redacted["exit_code"]
    report = {
        "profile": args.profile,
        "verdict": redacted["validation"]["verdict"],
        "summary": redacted["summary"],
        "validation": redacted["validation"],
        "redaction": {
            "applied": redacted["redaction"]["applied"],
            "redacted_count": redacted["redaction"]["redacted_count"],
        },
    }
    _print_summary(report, args.quiet, out_path)
    return code


if __name__ == "__main__":
    sys.exit(main())
