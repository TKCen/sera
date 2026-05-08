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
/// to probe the boundary. Pinned to an immutable digest so upstream BusyBox
/// behaviour changes can never silently flip the test outcome; bump
/// deliberately when a Docker Hub re-tag actually breaks the asserts.
/// (Tag retained alongside the digest purely for human readability — Docker
/// resolves by digest when both are present.)
const HARNESS_IMAGE: &str =
    "alpine:3@sha256:5b10f432ef3da1b8d4c7eb6c487f2f5a8f096bc91145e68878dd4a5019afde11";

/// Gateway-stand-in image — any HTTP server that returns a recognisable
/// body is sufficient. nginx:alpine's default index ships an HTML body
/// containing both `<html>` and `nginx`, which we assert on. Pinned to an
/// immutable digest for the same reason as [`HARNESS_IMAGE`].
const GATEWAY_IMAGE: &str =
    "nginx:alpine@sha256:5616878291a2eed594aee8db4dade5878cf7edcb475e59193904b198d9b830de";

/// Tear-down guard for a per-test Docker bridge. Runs `docker rm -f` for
/// the gateway-stand-in and any other active endpoints, then removes the
/// network on `Drop` so a panicking/timed-out test still removes its bridge.
/// All commands are best-effort — failures are swallowed so the guard is
/// safe to drop multiple times or against partially-removed resources.
struct BridgeGuard {
    network: String,
    container: String,
}

impl Drop for BridgeGuard {
    fn drop(&mut self) {
        remove_container(&self.container);
        remove_attached_containers(&self.network);
        for _ in 0..10 {
            let removed = Command::new("docker")
                .args(["network", "rm", &self.network])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|status| status.success())
                .unwrap_or(false);
            if removed {
                return;
            }
            remove_attached_containers(&self.network);
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn remove_container(container: &str) {
    let _ = Command::new("docker")
        .args(["rm", "-f", container])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn remove_attached_containers(network: &str) {
    let out = Command::new("docker")
        .args([
            "network",
            "inspect",
            "--format",
            "{{range $id, $_ := .Containers}}{{println $id}}{{end}}",
            network,
        ])
        .output();
    let Ok(out) = out else {
        return;
    };
    if !out.status.success() {
        return;
    }
    for container_id in String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        remove_container(container_id);
    }
}

fn assert_network_removed(network: &str) {
    let inspect = Command::new("docker")
        .args(["network", "inspect", network])
        .output()
        .expect("spawn docker network inspect");
    assert!(
        !inspect.status.success(),
        "bridge guard should remove network {network}; inspect stdout: {:?}, stderr: {:?}",
        String::from_utf8_lossy(&inspect.stdout),
        String::from_utf8_lossy(&inspect.stderr)
    );
}

fn docker_available() -> bool {
    Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

#[test]
fn bridge_guard_removes_extra_active_endpoints_before_network_rm() {
    if !docker_available() {
        skip(
            "bridge_guard_removes_extra_active_endpoints_before_network_rm",
            "docker unavailable",
        );
        return;
    }
    if let Err(e) = pull_image(HARNESS_IMAGE) {
        skip(
            "bridge_guard_removes_extra_active_endpoints_before_network_rm",
            format!("harness image unavailable: {e}"),
        );
        return;
    }

    let suffix = Uuid::new_v4().to_string();
    let network = format!("sera-eq0m-bridge-guard-cleanup-{suffix}");
    let extra_container = format!("{network}-extra");

    let net = Command::new("docker")
        .args([
            "network",
            "create",
            "--internal",
            "--driver=bridge",
            &network,
        ])
        .output()
        .expect("spawn docker network create");
    assert!(
        net.status.success(),
        "docker network create failed: {}",
        String::from_utf8_lossy(&net.stderr)
    );

    let run = Command::new("docker")
        .args([
            "run",
            "-d",
            "--rm",
            "--name",
            &extra_container,
            "--network",
            &network,
            HARNESS_IMAGE,
            "sh",
            "-c",
            "sleep 60",
        ])
        .output()
        .expect("spawn docker run extra endpoint");
    if !run.status.success() {
        let _ = Command::new("docker")
            .args(["network", "rm", &network])
            .status();
        panic!(
            "docker run extra endpoint failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }

    drop(BridgeGuard {
        network: network.clone(),
        container: format!("{network}-missing-gateway"),
    });

    let container_inspect = Command::new("docker")
        .args(["container", "inspect", &extra_container])
        .output()
        .expect("spawn docker container inspect");
    assert!(
        !container_inspect.status.success(),
        "bridge guard should remove extra endpoint container {extra_container}"
    );

    let network_inspect = Command::new("docker")
        .args(["network", "inspect", &network])
        .output()
        .expect("spawn docker network inspect");
    assert!(
        !network_inspect.status.success(),
        "bridge guard should remove network {network} after endpoints are gone"
    );
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

    // IP allocation precedes nginx accepting connections by a variable
    // window — sub-second on a warm host, multiple seconds on a cold CI
    // runner. Without this poll, tests 2 and 3's positive control could
    // flake with ECONNREFUSED for reasons unrelated to egress policy. Run
    // a disposable probe container on the same bridge that loops
    // `wget http://<gw-ip>/` until it gets a response or gives up.
    wait_for_gateway_ready(&network, &gateway_ip)?;

    Ok((
        guard,
        EgressBridgeConfig {
            network_name: network,
            gateway_target: gateway_ip,
        },
    ))
}

/// Block until the gateway-stand-in is accepting HTTP connections on its
/// allocated IP, or up to ~6 s. Uses a one-shot disposable container on
/// the same bridge so the check is image-agnostic (no nginx-specific log
/// scraping) and works whatever HTTP server the gateway image runs.
fn wait_for_gateway_ready(network: &str, gateway_ip: &str) -> Result<(), String> {
    let probe_cmd = format!(
        "for i in $(seq 1 30); do \
            wget -q -O /dev/null --timeout=1 http://{gateway_ip}/ && exit 0; \
            sleep 0.2; \
         done; exit 1"
    );
    let out = Command::new("docker")
        .args([
            "run",
            "--rm",
            "--network",
            network,
            HARNESS_IMAGE,
            "sh",
            "-c",
            &probe_cmd,
        ])
        .output()
        .map_err(|e| format!("docker run readiness probe spawn: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "gateway readiness probe at http://{gateway_ip}/ on bridge {network} \
             did not respond within ~6 s; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
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

#[derive(Clone, Copy, Debug)]
struct ForbiddenEgressTarget {
    name: &'static str,
    url: &'static str,
    marker: &'static str,
}

fn forbidden_egress_targets() -> &'static [ForbiddenEgressTarget] {
    &[
        ForbiddenEgressTarget {
            name: "metadata",
            url: "http://169.254.169.254/latest/meta-data/",
            marker: "metadata_exit=",
        },
        ForbiddenEgressTarget {
            name: "link_local_ipv4",
            url: "http://169.254.1.1/",
            marker: "link_local_ipv4_exit=",
        },
        ForbiddenEgressTarget {
            name: "rfc1918_10",
            url: "http://10.0.0.1/",
            marker: "rfc1918_10_exit=",
        },
        ForbiddenEgressTarget {
            name: "rfc1918_172",
            url: "http://172.16.0.1/",
            marker: "rfc1918_172_exit=",
        },
        ForbiddenEgressTarget {
            name: "rfc1918_192",
            url: "http://192.168.0.1/",
            marker: "rfc1918_192_exit=",
        },
        ForbiddenEgressTarget {
            name: "ipv6_loopback",
            url: "http://[::1]/",
            marker: "ipv6_loopback_exit=",
        },
        ForbiddenEgressTarget {
            name: "ipv6_private_ula",
            url: "http://[fc00::1]/",
            marker: "ipv6_private_ula_exit=",
        },
        ForbiddenEgressTarget {
            name: "ipv6_link_local_scoped",
            // IPv6 link-local probes must include an interface scope to be
            // routable; the percent-encoded zone id keeps this URL valid in
            // wget/curl parsers while still exercising the policy path.
            url: "http://[fe80::1%25eth0]/",
            marker: "ipv6_link_local_scoped_exit=",
        },
    ]
}

fn denied_probe_script(targets: &[ForbiddenEgressTarget]) -> String {
    let mut script = String::new();
    for target in targets {
        script.push_str(&format!(
            "timeout 4 wget -q -O- -T 2 {} >/tmp/{}_body 2>&1; echo \"{}$?\"; ",
            target.url, target.name, target.marker
        ));
    }
    script.push_str(
        "wget -q -O- --timeout=5 http://inference.local/ 2>&1; echo \"inference_exit=$?\"",
    );
    script
}

/// Pull both required images. Returns `Err` if either pull fails so the
/// caller can `skip()` the test rather than panic — a freshly-cloned dev
/// box without internet still passes `cargo test`.
fn ensure_images() -> Result<(), String> {
    pull_image(HARNESS_IMAGE)?;
    pull_image(GATEWAY_IMAGE)?;
    Ok(())
}

/// Pull a `<marker>N` line out of the captured stdout. The `docker run`
/// exit code itself is unreliable as a denial signal (the outer `sh -c`
/// always exits 0 because of the trailing `echo`), so the test contract is
/// "did the inner probe report a non-zero exit under this named marker?"
///
/// Tests use distinct prefixes (e.g. `anthropic_exit=`, `inference_exit=`)
/// when a single sandbox run probes more than one destination, so the
/// individual results can be asserted on independently.
fn parse_marker(stdout: &str, marker: &str) -> Option<i32> {
    stdout
        .lines()
        .filter_map(|l| l.strip_prefix(marker))
        .next()
        .and_then(|v| v.trim().parse::<i32>().ok())
}

#[test]
fn sv76_3_forbidden_target_matrix_covers_metadata_private_link_local_and_ipv6() {
    let targets = forbidden_egress_targets();
    let urls = targets.iter().map(|t| t.url).collect::<Vec<_>>();

    assert!(urls.contains(&"http://169.254.169.254/latest/meta-data/"));
    assert!(urls.contains(&"http://169.254.1.1/"));
    assert!(urls.contains(&"http://10.0.0.1/"));
    assert!(urls.contains(&"http://172.16.0.1/"));
    assert!(urls.contains(&"http://192.168.0.1/"));
    assert!(urls.contains(&"http://[::1]/"));
    assert!(urls.contains(&"http://[fc00::1]/"));
    assert!(urls.contains(&"http://[fe80::1]/"));
    assert!(targets.iter().all(|t| t.marker.ends_with("_exit=")));
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
    let (guard, bridge) = match provision_bridge(test_name) {
        Ok(v) => v,
        Err(e) => {
            skip(test_name, format!("bridge provisioning: {e}"));
            return;
        }
    };
    let network_name = bridge.network_name.clone();

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
    drop(guard);
    assert_network_removed(&network_name);

    let inner_exit = parse_marker(&result.stdout, "exit=").unwrap_or_else(|| {
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
    let (guard, bridge) = match provision_bridge(test_name) {
        Ok(v) => v,
        Err(e) => {
            skip(test_name, format!("bridge provisioning: {e}"));
            return;
        }
    };
    let network_name = bridge.network_name.clone();

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
    drop(guard);
    assert_network_removed(&network_name);

    let inner_exit = parse_marker(&result.stdout, "exit=").unwrap_or_else(|| {
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
// Sets `ANTHROPIC_API_KEY=sk-leaked-...` in the sandbox env, then runs two
// probes inside one sandbox:
//
//   * `nslookup api.anthropic.com` — would resolve to a real public IP on
//     an unrestricted Docker bridge (exit=0) and SERVFAIL on `--internal`
//     (exit≠0). This is the *denial* assertion: a regression that removed
//     strict-mode enforcement would observably succeed here, where the
//     earlier wget-against-application-endpoint probe could falsely pass
//     for application-layer reasons (HTTPS-only host, 401 auth, busybox
//     wget without TLS).
//   * `wget http://inference.local/` — *positive control*. Proves the
//     bridge is wired and the sandbox is not simply offline; an
//     accidentally-isolated harness with no networking at all would fail
//     this probe too, which would obscure the meaning of the first
//     assertion.
//
// The env-var visibility check (`echo key=$ANTHROPIC_API_KEY`) confirms
// the leak was actually present, so the "stale key cannot bypass" claim
// is meaningful rather than vacuously true.

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
    let (guard, bridge) = match provision_bridge(test_name) {
        Ok(v) => v,
        Err(e) => {
            skip(test_name, format!("bridge provisioning: {e}"));
            return;
        }
    };
    let network_name = bridge.network_name.clone();

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
             nslookup api.anthropic.com 2>&1; \
             echo \"anthropic_exit=$?\"; \
             wget -q -O- --timeout=5 http://inference.local/ 2>&1; \
             echo \"inference_exit=$?\"",
            &HashMap::new(),
        )
        .await
        .expect("execute");
    provider.destroy(&handle).await.expect("destroy");
    drop(guard);
    assert_network_removed(&network_name);

    assert!(
        result.stdout.contains(&format!("key={LEAKED_KEY}")),
        "ANTHROPIC_API_KEY must be visible inside the sandbox (proof the env \
         var was actually present, so the denial is meaningful) — stdout: {:?}",
        result.stdout
    );

    let anthropic_exit = parse_marker(&result.stdout, "anthropic_exit=").unwrap_or_else(|| {
        panic!(
            "stdout must contain `anthropic_exit=N` marker; got stdout: {:?}, stderr: {:?}",
            result.stdout, result.stderr
        )
    });
    assert_ne!(
        anthropic_exit, 0,
        "nslookup of api.anthropic.com on --internal bridge must fail even with a \
         stale ANTHROPIC_API_KEY in env; would succeed on a normal bridge so this \
         is the denial signal — stdout: {:?}, stderr: {:?}",
        result.stdout, result.stderr
    );

    let inference_exit = parse_marker(&result.stdout, "inference_exit=").unwrap_or_else(|| {
        panic!(
            "stdout must contain `inference_exit=N` marker; got stdout: {:?}, stderr: {:?}",
            result.stdout, result.stderr
        )
    });
    assert_eq!(
        inference_exit, 0,
        "positive control: inference.local must remain reachable (proves the \
         sandbox is not vacuously offline; without this, the anthropic-denied \
         assertion could pass for the wrong reason) — stdout: {:?}, stderr: {:?}",
        result.stdout, result.stderr
    );
    let body = result.stdout.to_ascii_lowercase();
    assert!(
        body.contains("html") || body.contains("nginx"),
        "positive control body must come from the nginx gateway-stand-in; \
         stdout: {:?}",
        result.stdout
    );
}

// ── Test 4 — metadata/private/link-local/IPv6 targets denied ───────────────
//
// This expands the original public-provider denial beyond DNS-only targets.
// Every forbidden URL must fail on the strict internal bridge, while
// inference.local remains reachable as a positive control. A leaked provider
// key is present to prove credentials do not bypass the kernel-layer boundary.

#[tokio::test]
async fn sv76_3_strict_blocks_metadata_private_link_local_and_ipv6_targets() {
    let test_name = "sv76-3-egress-expanded";
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
    let (guard, bridge) = match provision_bridge(test_name) {
        Ok(v) => v,
        Err(e) => {
            skip(test_name, format!("bridge provisioning: {e}"));
            return;
        }
    };
    let network_name = bridge.network_name.clone();

    const LEAKED_KEY: &str = "sk-expanded...deny";
    let mut env = HashMap::new();
    env.insert("OPENAI_API_KEY".to_string(), LEAKED_KEY.to_string());
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
            &denied_probe_script(forbidden_egress_targets()),
            &HashMap::new(),
        )
        .await
        .expect("execute");
    provider.destroy(&handle).await.expect("destroy");
    drop(guard);
    assert_network_removed(&network_name);

    for target in forbidden_egress_targets() {
        let inner_exit = parse_marker(&result.stdout, target.marker).unwrap_or_else(|| {
            panic!(
                "stdout must contain `{}` marker for {}; got stdout: {:?}, stderr: {:?}",
                target.marker, target.url, result.stdout, result.stderr
            )
        });
        assert_ne!(
            inner_exit, 0,
            "strict bridge must deny {}; got inner exit=0 — stdout: {:?}, stderr: {:?}",
            target.url, result.stdout, result.stderr
        );
    }

    let inference_exit = parse_marker(&result.stdout, "inference_exit=").unwrap_or_else(|| {
        panic!(
            "stdout must contain `inference_exit=N` marker; got stdout: {:?}, stderr: {:?}",
            result.stdout, result.stderr
        )
    });
    assert_eq!(
        inference_exit, 0,
        "positive control: inference.local must remain reachable — stdout: {:?}, stderr: {:?}",
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
