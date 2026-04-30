//! Acceptance tests for the `sera-eq0m` Docker-bridge egress boundary.
//!
//! These tests exercise [`DockerSandboxProvider`]'s `EgressManifest::Strict`
//! enforcement (introduced under `sera-jpdu` / PR #1133) end-to-end on a
//! real Docker daemon and prove three properties:
//!
//! 1. A sandbox attached to a strict-egress bridge cannot resolve / dial
//!    `api.anthropic.com`.
//! 2. The same sandbox can reach a wiremock-style "gateway" container via
//!    `--add-host=inference.local:<gw>` over an `--internal` Docker bridge.
//! 3. A leaked `ANTHROPIC_API_KEY` in the sandbox env does not let the
//!    harness bypass the kernel-layer denial — the env var is observable
//!    inside the container, but the connect(2) still fails.
//!
//! ## What this layer asserts (and what is *not* asserted yet)
//!
//! The bead's full acceptance contract additionally requires "exactly one
//! audit row recorded for the harness principal" (test 2) and "no audit
//! row for the denied dials" (tests 1 & 3). The gateway proxy that would
//! persist those rows is itself a follow-up bead — see PR B in
//! `artifacts/reports/research/eq0m-egress-restriction-scout-2026-04-30.md`:
//! the merged `/v1/chat/completions` and `/v1/messages` routes do not yet
//! emit `inference_audit` rows. Asserting on that table here would silently
//! pass against an empty schema and create a false-green compliance signal,
//! so the audit-row half of the contract is left to a later test once the
//! gateway proxy persists rows.
//!
//! These tests therefore cover the *kernel-layer* half of the acceptance
//! contract — the half that is enforceable today.
//!
//! ## CI safety
//!
//! - All denial happens via `docker network create --internal`, never via
//!   host iptables / nftables.
//! - Each test creates a uniquely-named bridge + gateway-stand-in container
//!   and tears them down in a `Drop` guard; teardown is best-effort and
//!   re-entrant.
//! - When the docker daemon is unreachable or the public images cannot be
//!   pulled (no internet on the runner), each test emits a `SKIP` line and
//!   returns without panicking.

#![cfg(feature = "docker")]

use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::time::Duration;

use sera_tools::sandbox::{
    EgressBridgeConfig, EgressEndpoint, EgressManifest, EgressMode, SandboxConfig, SandboxProvider,
    docker::DockerSandboxProvider,
};
use uuid::Uuid;

/// Harness-side image — busybox `nslookup` + `wget` + `sh` are all we need
/// to probe the boundary. `alpine:3` is small and ubiquitous on CI runners.
const HARNESS_IMAGE: &str = "alpine:3";

/// Gateway-stand-in image — any HTTP server that returns a recognisable
/// body is sufficient. nginx:alpine's default index ships an HTML body
/// containing both `<html>` and `nginx`, which we assert on.
const GATEWAY_IMAGE: &str = "nginx:alpine";

/// Tear-down guard for a (network, gateway-stand-in) pair. Runs `docker rm
/// -f` + `docker network rm` on `Drop` so a panicking test still removes
/// its bridge. Both commands are best-effort — failures are swallowed so
/// the guard is safe to drop multiple times or against partially-removed
/// resources.
struct BridgeGuard {
    network: String,
    container: String,
}

impl Drop for BridgeGuard {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.container])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = Command::new("docker")
            .args(["network", "rm", &self.network])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn docker_available() -> bool {
    Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

fn pull_image(image: &str) -> Result<(), String> {
    let out = Command::new("docker")
        .args(["pull", image])
        .output()
        .map_err(|e| format!("docker pull spawn: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "docker pull {image} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

/// Provision a `--internal` bridge + gateway-stand-in container and return
/// the egress bridge config + a tear-down guard.
///
/// `--internal` is what gives tests 1 and 3 their "denied" property without
/// any host firewall mutation: the embedded Docker DNS resolver cannot
/// upstream-resolve public names, so `nslookup api.anthropic.com` returns
/// SERVFAIL and any `wget http://api.anthropic.com/` reports `bad address`.
/// `--add-host=inference.local:<gw-ip>` (applied by `DockerSandboxProvider`
/// from the returned [`EgressBridgeConfig`]) is what gives test 2 its
/// routing.
fn provision_bridge(test_name: &str) -> Result<(BridgeGuard, EgressBridgeConfig), String> {
    let suffix = Uuid::new_v4().to_string();
    let network = format!("sera-eq0m-{test_name}-{suffix}");
    let container = format!("{network}-gw");

    let net = Command::new("docker")
        .args([
            "network",
            "create",
            "--internal",
            "--driver=bridge",
            &network,
        ])
        .output()
        .map_err(|e| format!("docker network create spawn: {e}"))?;
    if !net.status.success() {
        return Err(format!(
            "docker network create failed: {}",
            String::from_utf8_lossy(&net.stderr)
        ));
    }

    // The gateway-stand-in must be pre-pulled (see [`pull_image`] below) —
    // attaching it to the `--internal` bridge means the daemon cannot pull
    // missing images while creating the container.
    let run = Command::new("docker")
        .args([
            "run",
            "-d",
            "--rm",
            "--name",
            &container,
            "--network",
            &network,
            GATEWAY_IMAGE,
        ])
        .output()
        .map_err(|e| format!("docker run spawn: {e}"))?;
    if !run.status.success() {
        // Network was created but the gateway didn't start — clean up the
        // network now since there's no guard yet to do it on drop.
        let _ = Command::new("docker")
            .args(["network", "rm", &network])
            .status();
        return Err(format!(
            "docker run gateway failed: {}",
            String::from_utf8_lossy(&run.stderr)
        ));
    }

    let guard = BridgeGuard {
        network: network.clone(),
        container: container.clone(),
    };

    // Resolve the gateway's IP on the bridge. Allocation is synchronous
    // with `docker run`, but inspect occasionally lags by a few hundred
    // milliseconds; poll for up to ~6 s.
    let inspect_fmt = format!("{{{{(index .NetworkSettings.Networks \"{network}\").IPAddress}}}}");
    let mut gateway_ip = String::new();
    for _ in 0..30 {
        let ins = Command::new("docker")
            .args(["inspect", "-f", &inspect_fmt, &container])
            .output()
            .map_err(|e| format!("docker inspect spawn: {e}"))?;
        if ins.status.success() {
            let v = String::from_utf8_lossy(&ins.stdout).trim().to_string();
            if !v.is_empty() && v != "<no value>" {
                gateway_ip = v;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if gateway_ip.is_empty() {
        // Guard's drop will clean up the partial state.
        return Err(format!(
            "gateway container has no IP on bridge {network}; cleanup will run on guard drop"
        ));
    }

    Ok((
        guard,
        EgressBridgeConfig {
            network_name: network,
            gateway_target: gateway_ip,
        },
    ))
}

fn skip(test: &str, reason: impl AsRef<str>) {
    eprintln!("[{test}] SKIP: {}", reason.as_ref());
}

fn strict_inference_local() -> EgressManifest {
    EgressManifest {
        allowed: vec![EgressEndpoint::InferenceLocal],
        mode: EgressMode::Strict,
    }
}

/// Pull both required images. Returns `Err` if either pull fails so the
/// caller can `skip()` the test rather than panic — a freshly-cloned dev
/// box without internet still passes `cargo test`.
fn ensure_images() -> Result<(), String> {
    pull_image(HARNESS_IMAGE)?;
    pull_image(GATEWAY_IMAGE)?;
    Ok(())
}

/// Pull the stdout `exit=N` marker we append to every probe command. The
/// `docker run` exit code itself is unreliable as a denial signal (the
/// outer `sh -c` always exits 0 because of the trailing `echo`), so the
/// test contract is "did the inner probe report a non-zero exit?"
fn parse_inner_exit(stdout: &str) -> Option<i32> {
    stdout
        .lines()
        .filter_map(|l| l.strip_prefix("exit="))
        .next()
        .and_then(|v| v.trim().parse::<i32>().ok())
}

// ── Test 1 — direct dial denied ─────────────────────────────────────────────
//
// The bridge is `--internal`, so the embedded resolver cannot reach
// upstream DNS. `nslookup api.anthropic.com` returns SERVFAIL with a
// non-zero exit. The test asserts on the captured inner exit value rather
// than the outer `docker run` exit (the outer `sh -c "...; echo exit=$?"`
// always exits 0).

#[tokio::test]
async fn eq0m_test1_strict_blocks_direct_anthropic_dial() {
    let test_name = "eq0m-1";
    if !docker_available() {
        skip(test_name, "docker daemon not reachable");
        return;
    }
    if let Err(e) = ensure_images() {
        skip(test_name, format!("image pull: {e}"));
        return;
    }
    let Ok(provider) = DockerSandboxProvider::new() else {
        skip(test_name, "DockerSandboxProvider::new failed");
        return;
    };
    let (_guard, bridge) = match provision_bridge(test_name) {
        Ok(v) => v,
        Err(e) => {
            skip(test_name, format!("bridge provisioning: {e}"));
            return;
        }
    };

    let config = SandboxConfig {
        image: Some(HARNESS_IMAGE.to_string()),
        egress: Some(strict_inference_local()),
        egress_bridge: Some(bridge),
        ..Default::default()
    };
    let handle = provider.create(&config).await.expect("create");
    let result = provider
        .execute(
            &handle,
            "nslookup api.anthropic.com 2>&1; echo \"exit=$?\"",
            &HashMap::new(),
        )
        .await
        .expect("execute");
    provider.destroy(&handle).await.expect("destroy");

    let inner_exit = parse_inner_exit(&result.stdout).unwrap_or_else(|| {
        panic!(
            "stdout must contain `exit=N` marker; got stdout: {:?}, stderr: {:?}",
            result.stdout, result.stderr
        )
    });
    assert_ne!(
        inner_exit, 0,
        "nslookup of api.anthropic.com on a --internal bridge must fail; \
         got inner exit=0 (egress not denied) — stdout: {:?}, stderr: {:?}",
        result.stdout, result.stderr
    );
}

// ── Test 2 — inference.local routes to the gateway-stand-in ────────────────
//
// `--add-host=inference.local:<gw-ip>` maps the virtual host to the
// gateway-stand-in's IP on the bridge. `wget http://inference.local/` hits
// nginx, which returns its default index containing both `<html>` and
// `nginx`. Any successful body proves routing worked, since the bridge is
// internal and no other host on it could serve the request.

#[tokio::test]
async fn eq0m_test2_strict_allows_inference_local_to_gateway() {
    let test_name = "eq0m-2";
    if !docker_available() {
        skip(test_name, "docker daemon not reachable");
        return;
    }
    if let Err(e) = ensure_images() {
        skip(test_name, format!("image pull: {e}"));
        return;
    }
    let Ok(provider) = DockerSandboxProvider::new() else {
        skip(test_name, "DockerSandboxProvider::new failed");
        return;
    };
    let (_guard, bridge) = match provision_bridge(test_name) {
        Ok(v) => v,
        Err(e) => {
            skip(test_name, format!("bridge provisioning: {e}"));
            return;
        }
    };

    let config = SandboxConfig {
        image: Some(HARNESS_IMAGE.to_string()),
        egress: Some(strict_inference_local()),
        egress_bridge: Some(bridge),
        ..Default::default()
    };
    let handle = provider.create(&config).await.expect("create");
    let result = provider
        .execute(
            &handle,
            "wget -q -O- --timeout=5 http://inference.local/ 2>&1; echo \"exit=$?\"",
            &HashMap::new(),
        )
        .await
        .expect("execute");
    provider.destroy(&handle).await.expect("destroy");

    let inner_exit = parse_inner_exit(&result.stdout).unwrap_or_else(|| {
        panic!(
            "stdout must contain `exit=N` marker; got stdout: {:?}, stderr: {:?}",
            result.stdout, result.stderr
        )
    });
    assert_eq!(
        inner_exit, 0,
        "wget of inference.local must succeed (inner exit=0); got stdout: {:?}, stderr: {:?}",
        result.stdout, result.stderr
    );
    let body = result.stdout.to_ascii_lowercase();
    assert!(
        body.contains("html") || body.contains("nginx"),
        "wget body must come from the nginx gateway-stand-in (HTML/nginx marker); \
         stdout: {:?}",
        result.stdout
    );
}

// ── Test 3 — stale ANTHROPIC_API_KEY does not bypass denial ────────────────
//
// Sets `ANTHROPIC_API_KEY=sk-leaked-...` in the sandbox env, then probes
// api.anthropic.com using that key. The boundary holds because connect(2)
// fails before any auth bytes leave the sandbox. The test additionally
// asserts the env var was actually visible inside the container, so the
// "stale key cannot bypass" claim is meaningful (rather than vacuously
// true because the key was missing).

#[tokio::test]
async fn eq0m_test3_strict_ignores_stale_anthropic_api_key_env() {
    let test_name = "eq0m-3";
    if !docker_available() {
        skip(test_name, "docker daemon not reachable");
        return;
    }
    if let Err(e) = ensure_images() {
        skip(test_name, format!("image pull: {e}"));
        return;
    }
    let Ok(provider) = DockerSandboxProvider::new() else {
        skip(test_name, "DockerSandboxProvider::new failed");
        return;
    };
    let (_guard, bridge) = match provision_bridge(test_name) {
        Ok(v) => v,
        Err(e) => {
            skip(test_name, format!("bridge provisioning: {e}"));
            return;
        }
    };

    const LEAKED_KEY: &str = "sk-leaked-eq0m-test3-must-not-bypass";
    let mut env = HashMap::new();
    env.insert("ANTHROPIC_API_KEY".to_string(), LEAKED_KEY.to_string());
    let config = SandboxConfig {
        image: Some(HARNESS_IMAGE.to_string()),
        egress: Some(strict_inference_local()),
        egress_bridge: Some(bridge),
        env,
        ..Default::default()
    };
    let handle = provider.create(&config).await.expect("create");
    let result = provider
        .execute(
            &handle,
            "echo \"key=$ANTHROPIC_API_KEY\"; \
             wget -q -T 3 -O- --header=\"x-api-key: $ANTHROPIC_API_KEY\" \
             http://api.anthropic.com/v1/messages 2>&1; \
             echo \"exit=$?\"",
            &HashMap::new(),
        )
        .await
        .expect("execute");
    provider.destroy(&handle).await.expect("destroy");

    assert!(
        result.stdout.contains(&format!("key={LEAKED_KEY}")),
        "ANTHROPIC_API_KEY must be visible inside the sandbox (proof the env \
         var was actually present, so the denial is meaningful) — stdout: {:?}",
        result.stdout
    );
    let inner_exit = parse_inner_exit(&result.stdout).unwrap_or_else(|| {
        panic!(
            "stdout must contain `exit=N` marker; got stdout: {:?}, stderr: {:?}",
            result.stdout, result.stderr
        )
    });
    assert_ne!(
        inner_exit, 0,
        "wget of api.anthropic.com on --internal bridge must fail even with a \
         stale ANTHROPIC_API_KEY in env; stdout: {:?}, stderr: {:?}",
        result.stdout, result.stderr
    );
}

// ── Acceptance criterion 4 — bridge teardown is idempotent ────────────────
//
// `BridgeGuard::drop` runs `docker rm -f` + `docker network rm` against
// names that may already be gone (e.g. when a previous test panicked
// between provision and drop). The guard must not panic in those cases.
// This test exercises that path directly without spawning a real bridge.

#[tokio::test]
async fn eq0m_bridge_guard_drop_is_safe_when_resources_already_removed() {
    let suffix = Uuid::new_v4().to_string();
    let guard = BridgeGuard {
        network: format!("sera-eq0m-no-such-net-{suffix}"),
        container: format!("sera-eq0m-no-such-ct-{suffix}"),
    };
    drop(guard);
}
