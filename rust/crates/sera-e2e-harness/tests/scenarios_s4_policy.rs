//! S4 — Security / policy scenarios (Phase 2 of the TEST-SCENARIOS plan).
//!
//! These tests prove the gateway's enforcement surfaces do what the spec
//! says.  Phase 2 opens with S4.3 KillSwitch: the rest of S4 (CapabilityPolicy
//! deny, SSRF at integration level, Constitutional gate) is tracked separately
//! because it requires more fixture setup (policy files, tool-call-emitting
//! mock LLM).
//!
//! Covered in this file:
//! * S4.3 — arming the KillSwitch via `ROLLBACK` on the admin socket makes
//!   all non-health HTTP requests return 503, while `/api/health` continues
//!   to respond.  This is the first-line operator panic button — if it
//!   regresses, no amount of in-flight cancellation machinery matters.

#![cfg(all(unix, feature = "integration"))]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use anyhow::{Context, Result};

use sera_e2e_harness::binaries::{gateway_bin, runtime_bin};
use sera_e2e_harness::mock_llm::start_mock_llm;
use sera_e2e_harness::InProcessGateway;

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SERA_E2E_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_test_writer()
        .try_init();
}

fn bins_or_skip() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    Some((gateway_bin()?, runtime_bin()?))
}

/// S4.3 — The KillSwitch admin-socket gate blocks subsequent non-health
/// requests after a `ROLLBACK` command.  Baseline request succeeds, rollback
/// is sent, next request returns 503, but `/api/health` still answers.
///
/// This exercises `kill_switch_gate` middleware + `spawn_admin_socket` +
/// `handle_command("ROLLBACK")` end-to-end through the real gateway binary.
/// The existing `killswitch_abort.rs` test covers the in-flight cancellation
/// path at unit scope; this scenario is the HTTP-surface counterpart.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s4_3_killswitch_rollback_blocks_new_requests() -> Result<()> {
    init_tracing();

    let (gw_bin, rt_bin) = match bins_or_skip() {
        Some(b) => b,
        None => {
            eprintln!("[S4.3] SKIP: gateway/runtime bins not built");
            return Ok(());
        }
    };
    let (llm, _mock) = match start_mock_llm().await {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[S4.3] SKIP: wiremock unavailable ({e})");
            return Ok(());
        }
    };

    // Per-test admin socket path — must be unique so concurrent test runs
    // don't collide on `/tmp/sera-admin.sock`.  `tempfile::tempdir()` is
    // enough: the dir's drop removes the socket file after the test.
    let sock_dir = tempfile::tempdir().context("creating sock tempdir")?;
    let sock_path = sock_dir.path().join("admin.sock");
    let sock_str = sock_path
        .to_str()
        .context("tempdir path is not UTF-8")?
        .to_owned();

    let gw = InProcessGateway::start_local_with_env(
        &gw_bin,
        &rt_bin,
        &llm,
        &[("SERA_ADMIN_SOCK", sock_str.as_str())],
    )
    .await
    .context("booting gateway for S4.3")?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    // ── Baseline: /api/agents must work before the rollback ──
    let before = http
        .get(format!("{}/api/agents", gw.base_url))
        .send()
        .await?;
    assert!(
        before.status().is_success(),
        "baseline /api/agents must be 2xx; got {}",
        before.status()
    );

    // ── Fire ROLLBACK over the admin socket ──
    //
    // The socket binds in a background task inside the gateway child process.
    // It is created during boot but may race with the first health probe; we
    // retry the connect a few times so a slow CI runner doesn't flake.
    let mut connected = None;
    for _ in 0..30 {
        match UnixStream::connect(&sock_path) {
            Ok(s) => {
                connected = Some(s);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    let mut sock =
        connected.context("admin socket did not come up within 3s of gateway boot")?;
    sock.write_all(b"ROLLBACK\n").context("writing ROLLBACK")?;
    sock.set_read_timeout(Some(Duration::from_secs(2))).ok();
    let mut resp = String::new();
    sock.read_to_string(&mut resp).context("reading admin socket response")?;
    assert!(
        resp.contains("ARMED") || resp.contains("OK"),
        "admin socket should acknowledge ROLLBACK; got {resp:?}"
    );

    // ── After rollback: /api/agents must be 503 ──
    //
    // The gateway's rollback handler cancels in-flight turns asynchronously;
    // the gate flag itself flips synchronously via `KillSwitch::arm()` in
    // `handle_command("ROLLBACK")`, so the next request should see 503
    // immediately.  Retry once against eventual consistency.
    let mut blocked_status: Option<reqwest::StatusCode> = None;
    for _ in 0..10 {
        let resp = http
            .get(format!("{}/api/agents", gw.base_url))
            .send()
            .await?;
        blocked_status = Some(resp.status());
        if resp.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        blocked_status,
        Some(reqwest::StatusCode::SERVICE_UNAVAILABLE),
        "after ROLLBACK /api/agents must be 503; got {blocked_status:?}"
    );

    // ── /api/health must still answer ──
    //
    // `kill_switch_gate` in the gateway binary excludes health endpoints so
    // readiness probes / orchestrators can observe the halted state without
    // getting 503s from every surface.
    let h = http
        .get(format!("{}/api/health", gw.base_url))
        .send()
        .await?;
    assert!(
        h.status().is_success(),
        "/api/health must remain 2xx after rollback; got {}",
        h.status()
    );

    if let Err(e) = gw.shutdown().await {
        eprintln!("gateway shutdown returned: {e}");
    }
    Ok(())
}
