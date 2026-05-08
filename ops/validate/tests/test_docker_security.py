"""Unit tests for sv76.3 Docker security overlay helpers."""

from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SV_PATH = ROOT / "sera-validate.py"
PROFILE_PATH = ROOT / "profiles" / "docker_security.py"
REPO_ROOT = ROOT.parents[1]
OVERLAY_PATH = ROOT / "docker-compose.security.yml"


def _load_module(name: str, path: Path) -> object:
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_profile_registered() -> None:
    sv = _load_module("sera_validate", SV_PATH)
    assert "docker_security" in sv.PROFILES


def test_security_overlay_file_contains_required_hardening_knobs() -> None:
    docker_security = _load_module("docker_security", PROFILE_PATH)
    text = OVERLAY_PATH.read_text(encoding="utf-8")

    findings = docker_security.check_overlay_text(text)

    assert findings["missing"] == []
    assert findings["present"] == [
        "no_new_privileges",
        "cap_drop_all",
        "read_only_rootfs",
        "tmpfs_tmp",
        "tmpfs_var_tmp",
        "memory_limit",
        "cpu_limit",
        "pids_limit",
        "published_ports_reset",
    ]


def test_ephemeral_compose_commands_use_base_overlay_project_and_cleanup_volume() -> None:
    docker_security = _load_module("docker_security", PROFILE_PATH)
    project = docker_security.security_project_name("run id/with spaces")

    up_cmd = docker_security.compose_command(REPO_ROOT, project, "up")
    down_cmd = docker_security.compose_command(REPO_ROOT, project, "down")

    assert project.startswith("sera-sec-run-id-with-spaces")
    assert up_cmd == [
        "docker",
        "compose",
        "-f",
        str(REPO_ROOT / "rust" / "docker-compose.sera.yml"),
        "-f",
        str(OVERLAY_PATH),
        "-p",
        project,
        "up",
        "-d",
        "--build",
        "--wait",
    ]
    assert down_cmd == [
        "docker",
        "compose",
        "-f",
        str(REPO_ROOT / "rust" / "docker-compose.sera.yml"),
        "-f",
        str(OVERLAY_PATH),
        "-p",
        project,
        "down",
        "-v",
        "--rmi",
        "local",
        "--remove-orphans",
    ]


def test_run_records_structured_skip_when_docker_is_unavailable() -> None:
    docker_security = _load_module("docker_security", PROFILE_PATH)

    def fake_run(_cmd: list[str], _timeout: int = 60):
        return docker_security.CommandResult(127, "", "docker: not found")

    summary, evidence, errors, warnings, cases = docker_security.run(
        repo_root=REPO_ROOT,
        run_compose=False,
        command_runner=fake_run,
    )

    assert errors == []
    assert any("docker unavailable" in warning for warning in warnings)
    assert summary["docker_security"]["docker_available"] is False
    prereq = next(case for case in cases if case["case_id"] == "DOCKER-PREREQ-01")
    assert prereq["skipped"] is True
    assert prereq["skip_reason"] == "docker_unavailable"
    assert evidence["docker_version"] is None


def test_run_redacts_compose_config_secret_like_environment_values() -> None:
    docker_security = _load_module("docker_security", PROFILE_PATH)

    def fake_run(cmd: list[str], _timeout: int = 60):
        if "version" in cmd:
            return docker_security.CommandResult(0, "24.0.0\n", "")
        return docker_security.CommandResult(
            0,
            "environment:\n  SERA_API_KEY: raw-secret-value\n  RUST_LOG: info\n",
            "",
        )

    _summary, evidence, errors, warnings, cases = docker_security.run(
        repo_root=REPO_ROOT,
        run_compose=False,
        command_runner=fake_run,
    )

    serialized = str({"evidence": evidence, "cases": cases})
    assert errors == []
    assert warnings == []
    assert "raw-secret-value" not in serialized
    assert "<redacted:compose-config-env>" in serialized


def test_compose_failure_warnings_are_redacted() -> None:
    docker_security = _load_module("docker_security", PROFILE_PATH)

    def fake_run(cmd: list[str], _timeout: int = 60):
        if "version" in cmd:
            return docker_security.CommandResult(0, "24.0.0\n", "")
        if "config" in cmd:
            return docker_security.CommandResult(
                1,
                "",
                "environment:\n  SERA_API_KEY: raw-config-secret\n",
            )
        if "up" in cmd:
            return docker_security.CommandResult(
                1,
                "container log\nSERA_ADMIN_TOKEN=raw-up-secret\n",
                "",
            )
        if "down" in cmd:
            return docker_security.CommandResult(
                1,
                "",
                "cleanup log\nSERA_SECRET: raw-down-secret\n",
            )
        return docker_security.CommandResult(0, "", "")

    _summary, _evidence, errors, warnings, _cases = docker_security.run(
        repo_root=REPO_ROOT,
        run_compose=True,
        command_runner=fake_run,
    )
    serialized = str({"errors": errors, "warnings": warnings})

    assert "raw-config-secret" not in serialized
    assert "raw-up-secret" not in serialized
    assert "raw-down-secret" not in serialized
    assert "<redacted:compose-config-env>" in serialized


def test_main_docker_security_run_compose_passes_explicit_ephemeral_flag(tmp_path, monkeypatch) -> None:
    sv = _load_module("sera_validate_for_docker_security_cli", SV_PATH)
    captured: dict[str, bool] = {}

    def fake_profile_docker_security(*, run_compose: bool = False):
        captured["run_compose"] = run_compose
        return (
            {"docker_security": {"cases_total": 0}},
            {"compose_up": None, "compose_down": None},
            [],
            [],
            [],
        )

    monkeypatch.setattr(sv, "profile_docker_security", fake_profile_docker_security)
    out = tmp_path / "docker-security.json"

    code = sv.main([
        "--profile",
        "docker_security",
        "--run-compose",
        "--out",
        str(out),
        "--quiet",
    ])

    assert code == sv.EXIT_PASS
    assert captured["run_compose"] is True
    assert out.exists()


def test_run_compose_cleanup_always_attempts_down_after_up_failure() -> None:
    docker_security = _load_module("docker_security", PROFILE_PATH)
    calls: list[list[str]] = []

    def fake_run(cmd: list[str], _timeout: int = 60):
        calls.append(cmd)
        if "version" in cmd:
            return docker_security.CommandResult(0, "24.0.0\n", "")
        if "up" in cmd:
            return docker_security.CommandResult(1, "", "overlay incompatible")
        return docker_security.CommandResult(0, "removed\n", "")

    summary, _evidence, errors, warnings, cases = docker_security.run(
        repo_root=REPO_ROOT,
        run_compose=True,
        run_id="cleanup-test",
        command_runner=fake_run,
    )

    assert summary["docker_security"]["cleanup_attempted"] is True
    assert any("overlay incompatible" in warning for warning in warnings)
    assert errors == []
    assert any(case["case_id"] == "DOCKER-COMPOSE-02" for case in cases)
    assert any("up" in cmd for cmd in calls)
    assert any("down" in cmd and "-v" in cmd for cmd in calls)
