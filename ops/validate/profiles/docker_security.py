"""Docker security overlay profile and ephemeral compose helpers (sera-sv76.3).

Stdlib-only. This module intentionally does not mutate host firewall state. The
only side-effectful mode is an explicitly requested ephemeral compose lifecycle;
it always attempts `down -v --remove-orphans` after `up`, even when startup fails.
"""

from __future__ import annotations

import re
import subprocess
import uuid
from pathlib import Path
from typing import Any, Callable, NamedTuple

_THIS = Path(__file__).resolve()
_OPS_VALIDATE = _THIS.parents[1]
_REPO_ROOT = _OPS_VALIDATE.parents[1]
_OVERLAY_REL = Path("ops/validate/docker-compose.security.yml")
_BASE_COMPOSE_REL = Path("rust/docker-compose.sera.yml")

REQUIRED_OVERLAY_MARKERS: list[tuple[str, str]] = [
    ("no_new_privileges", "no-new-privileges:true"),
    ("cap_drop_all", "cap_drop:"),
    ("read_only_rootfs", "read_only: true"),
    ("tmpfs_tmp", "/tmp:"),
    ("tmpfs_var_tmp", "/var/tmp:"),
    ("memory_limit", "mem_limit:"),
    ("cpu_limit", "cpus:"),
    ("pids_limit", "pids_limit:"),
    ("published_ports_reset", "ports: !reset []"),
]
_SECRET_CONFIG_LINE_RE = re.compile(
    r"(?im)^(?P<prefix>\s*[A-Z0-9_]*(?:API_KEY|TOKEN|SECRET)\s*[:=]\s*).*$"
)


class CommandResult(NamedTuple):
    returncode: int
    stdout: str
    stderr: str


def _run_command(cmd: list[str], timeout: int = 60) -> CommandResult:
    try:
        out = subprocess.run(
            cmd,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
        )
    except FileNotFoundError as exc:
        return CommandResult(127, "", str(exc))
    except subprocess.TimeoutExpired as exc:
        return CommandResult(
            124,
            (exc.stdout or "") if isinstance(exc.stdout, str) else "",
            f"timeout after {timeout}s",
        )
    return CommandResult(out.returncode, out.stdout, out.stderr)


def security_project_name(run_id: str | None = None) -> str:
    raw = run_id or uuid.uuid4().hex[:12]
    safe = re.sub(r"[^a-zA-Z0-9_.-]+", "-", raw).strip("-._").lower()
    if not safe:
        safe = uuid.uuid4().hex[:12]
    return f"sera-sec-{safe}"[:63]


def compose_command(repo_root: Path | str, project: str, action: str) -> list[str]:
    root = Path(repo_root)
    prefix = [
        "docker",
        "compose",
        "-f",
        str(root / _BASE_COMPOSE_REL),
        "-f",
        str(root / _OVERLAY_REL),
        "-p",
        project,
    ]
    if action == "up":
        return prefix + ["up", "-d", "--build", "--wait"]
    if action == "down":
        return prefix + ["down", "-v", "--rmi", "local", "--remove-orphans"]
    if action == "config":
        return prefix + ["config"]
    raise ValueError(f"unsupported compose action: {action}")


def check_overlay_text(text: str) -> dict[str, list[str]]:
    present: list[str] = []
    missing: list[str] = []
    for key, marker in REQUIRED_OVERLAY_MARKERS:
        if marker in text:
            present.append(key)
        else:
            missing.append(key)
    if "cap_drop:" in text and "ALL" not in text:
        if "cap_drop_all" in present:
            present.remove("cap_drop_all")
        if "cap_drop_all" not in missing:
            missing.append("cap_drop_all")
    return {"present": present, "missing": missing}


def _safe_tail(text: str, limit: int = 2000) -> str:
    """Return a bounded command-output tail with compose env secrets removed."""
    redacted = _SECRET_CONFIG_LINE_RE.sub(
        lambda m: f"{m.group('prefix')}<redacted:compose-config-env>", text or ""
    )
    return redacted[-limit:]


def _case(case_id: str, category: str, **kwargs: Any) -> dict[str, Any]:
    case: dict[str, Any] = {
        "case_id": case_id,
        "category": category,
        "live_safe": False,
        "ephemeral_only": True,
        "skipped": False,
        "skip_reason": None,
        "expected": None,
        "actual": None,
        "passed": None,
        "finding": None,
    }
    case.update(kwargs)
    return case


def run(
    *,
    repo_root: Path | str | None = None,
    run_compose: bool = False,
    run_id: str | None = None,
    command_runner: Callable[[list[str], int], CommandResult] = _run_command,
) -> tuple[dict[str, Any], dict[str, Any], list[str], list[str], list[dict[str, Any]]]:
    """Validate the Docker security overlay and optionally exercise compose.

    Returns ``(summary, evidence, errors, warnings, cases)`` for the generic
    ``sera-validate`` profile wrapper. Docker/compose unavailability is recorded
    as a structured skip + warning, not as a pass.
    """
    root = Path(repo_root) if repo_root is not None else _REPO_ROOT
    overlay_path = root / _OVERLAY_REL
    project = security_project_name(run_id)
    errors: list[str] = []
    warnings: list[str] = []
    cases: list[dict[str, Any]] = []
    cleanup_attempted = False

    evidence: dict[str, Any] = {
        "repo_root": str(root),
        "overlay_path": str(overlay_path),
        "project": project,
        "docker_version": None,
        "compose_up": None,
        "compose_down": None,
    }

    if not overlay_path.exists():
        errors.append(f"overlay_missing: {overlay_path}")
        cases.append(
            _case(
                "DOCKER-OVERLAY-01",
                "overlay-static",
                expected="overlay file exists with security hardening knobs",
                actual="missing",
                passed=False,
                finding=f"overlay missing: {overlay_path}",
            )
        )
        summary = _summary(False, cleanup_attempted, cases)
        return summary, evidence, errors, warnings, cases

    text = overlay_path.read_text(encoding="utf-8")
    overlay_check = check_overlay_text(text)
    overlay_passed = not overlay_check["missing"]
    if not overlay_passed:
        errors.append(f"overlay_missing_hardening: {overlay_check['missing']}")
    cases.append(
        _case(
            "DOCKER-OVERLAY-01",
            "overlay-static",
            expected=[k for k, _ in REQUIRED_OVERLAY_MARKERS],
            actual=overlay_check,
            live_safe=True,
            ephemeral_only=False,
            passed=overlay_passed,
            finding=None if overlay_passed else "missing required hardening markers",
        )
    )

    version = command_runner(["docker", "version", "--format", "{{.Server.Version}}"], 20)
    docker_available = version.returncode == 0 and bool(version.stdout.strip())
    if docker_available:
        evidence["docker_version"] = version.stdout.strip()
        cases.append(
            _case(
                "DOCKER-PREREQ-01",
                "docker-prereq",
                expected="docker daemon reachable",
                actual=version.stdout.strip(),
                live_safe=True,
                ephemeral_only=False,
                passed=True,
            )
        )
    else:
        warnings.append(f"docker unavailable; compose validation skipped: {version.stderr or version.stdout}")
        cases.append(
            _case(
                "DOCKER-PREREQ-01",
                "docker-prereq",
                expected="docker daemon reachable",
                actual={"returncode": version.returncode, "stderr": version.stderr},
                live_safe=True,
                ephemeral_only=False,
                skipped=True,
                skip_reason="docker_unavailable",
            )
        )
        summary = _summary(False, cleanup_attempted, cases)
        return summary, evidence, errors, warnings, cases

    config_result = command_runner(compose_command(root, project, "config"), 60)
    config_passed = config_result.returncode == 0
    if not config_passed:
        warnings.append(
            "docker compose config reported overlay incompatibility: "
            f"{config_result.stderr or config_result.stdout}"
        )
    cases.append(
        _case(
            "DOCKER-COMPOSE-01",
            "compose-config",
            expected="base compose + security overlay render successfully",
            actual={
                "returncode": config_result.returncode,
                "stdout": _safe_tail(config_result.stdout),
                "stderr": _safe_tail(config_result.stderr),
            },
            passed=config_passed,
            finding=None if config_passed else "overlay compose config incompatibility",
        )
    )

    if run_compose:
        up = command_runner(compose_command(root, project, "up"), 300)
        evidence["compose_up"] = {
            "returncode": up.returncode,
            "stdout": _safe_tail(up.stdout),
            "stderr": _safe_tail(up.stderr),
        }
        up_passed = up.returncode == 0
        if not up_passed:
            warnings.append(
                "docker compose security overlay startup finding: "
                f"{up.stderr or up.stdout}"
            )
        cases.append(
            _case(
                "DOCKER-COMPOSE-02",
                "compose-up",
                expected="ephemeral security overlay stack starts under hardening",
                actual=evidence["compose_up"],
                passed=up_passed,
                finding=None if up_passed else "overlay startup incompatibility",
            )
        )
        cleanup_attempted = True
        down = command_runner(compose_command(root, project, "down"), 120)
        evidence["compose_down"] = {
            "returncode": down.returncode,
            "stdout": _safe_tail(down.stdout),
            "stderr": _safe_tail(down.stderr),
        }
        down_passed = down.returncode == 0
        if not down_passed:
            errors.append(f"cleanup_failed: {down.stderr or down.stdout}")
        cases.append(
            _case(
                "DOCKER-CLEANUP-01",
                "compose-cleanup",
                expected="docker compose down -v --remove-orphans succeeds",
                actual=evidence["compose_down"],
                passed=down_passed,
                finding=None if down_passed else "ephemeral cleanup failed",
            )
        )

    summary = _summary(docker_available, cleanup_attempted, cases)
    return summary, evidence, errors, warnings, cases


def _summary(
    docker_available: bool,
    cleanup_attempted: bool,
    cases: list[dict[str, Any]],
) -> dict[str, Any]:
    return {
        "docker_security": {
            "docker_available": docker_available,
            "cleanup_attempted": cleanup_attempted,
            "cases_total": len(cases),
            "cases_passed": sum(1 for c in cases if c.get("passed") is True),
            "cases_failed": sum(1 for c in cases if c.get("passed") is False),
            "cases_skipped": sum(1 for c in cases if c.get("skipped")),
            "findings": [c["finding"] for c in cases if c.get("finding")],
        }
    }
