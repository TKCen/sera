//! sera-plcv AC5 — KillSwitch ROLLBACK reaps the runtime child and the
//! gateway resumes serving after DISARM.
//!
//! Boots a gateway child via the harness's local profile, sends a turn so a
//! `sera-runtime` child exists, snapshots that runtime PID, fires `ROLLBACK`
//! on the admin Unix socket, then asserts:
//!
//! 1. The pre-rollback runtime PIDs are no longer children of the gateway —
//!    `kill_for_rollback` killed and reaped them. This is the AC5 evidence
//!    for "stops the corresponding lane".
//! 2. The gateway process itself is still alive.
//! 3. After `DISARM`, a new submission is accepted within 5 s — AC5 evidence
//!    for "without corrupting the gateway session".
//!
//! What this test deliberately does NOT assert:
//!
//! * It does not assert that the gateway has zero descendant PIDs after the
//!   post-DISARM submission. Sending a new turn legitimately respawns the
//!   runtime child, so a "no children at all" check would conflate the
//!   killed pre-rollback child with the freshly spawned post-DISARM one.
//!   The pre-rollback PID-set comparison above is the semantically correct
//!   AC5 assertion.
//!
//! AC1 (`sera-gateway start --local` boots and `/api/health` returns 200) is
//! covered by `local_profile_turn.rs`; this file is focused on AC5.

#![cfg(all(unix, feature = "integration"))]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::timeout;

use sera_e2e_harness::binaries::{gateway_bin, runtime_bin};
use sera_e2e_harness::mock_llm::start_mock_llm;
use sera_e2e_harness::InProcessGateway;

/// Collect direct child PIDs of `parent_pid` by scanning `/proc/*/status` for
/// `PPid:`. Returns an empty vector when `/proc` is unreadable or the parent
/// has no live descendants.
fn collect_child_pids(parent_pid: u32) -> Vec<u32> {
    let mut children = Vec::new();
    let proc_path = Path::new("/proc");
    let entries = match std::fs::read_dir(proc_path) {
        Ok(e) => e,
        Err(_) => return children,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let pid: u32 = match name.to_string_lossy().parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        if pid == parent_pid {
            continue;
        }
        let status = match std::fs::read_to_string(format!("/proc/{pid}/status")) {
            Ok(s) => s,
            Err(_) => continue,
        };
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("PPid:") {
                if let Ok(ppid) = rest.trim().parse::<u32>()
                    && ppid == parent_pid
                {
                    children.push(pid);
                }
                break;
            }
        }
    }
    children
}

/// Direct `kill(pid, 0)` syscall: returns true while the process is alive.
///
/// Using the syscall instead of shelling out to `kill -0` avoids false-passing
/// in minimal CI/container images where the `kill` binary may be missing or
/// unrunnable: with the previous `Command::output().unwrap_or(false)`, a
/// launch error silently made this return `false`, vacuously passing the AC5
/// reap assertion. The syscall is always available on Unix and matches the
/// `libc_kill` FFI shim used elsewhere in this crate (see `lib.rs`).
fn pid_alive(pid: u32) -> bool {
    // SAFETY: `kill` is declared with the standard POSIX FFI signature; with
    // `sig = 0` it only performs the existence probe and does not deliver a
    // signal. Same pattern as `libc_kill` in `sera-e2e-harness/src/lib.rs`.
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    let rc = unsafe { kill(pid as i32, 0) };
    if rc == 0 {
        return true;
    }
    // ESRCH (3 on Linux/macOS/*BSD) means no such process. Any other errno
    // (e.g. EPERM) means the PID exists but we lack permission to signal it —
    // the process is still alive, so don't false-green the reap assertion.
    const ESRCH: i32 = 3;
    std::io::Error::last_os_error().raw_os_error() != Some(ESRCH)
}

fn bins_or_skip() -> Option<(PathBuf, PathBuf)> {
    Some((gateway_bin()?, runtime_bin()?))
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SERA_E2E_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_test_writer()
        .try_init();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ac5_kill_switch_rollback_reaps_runtime_and_resumes_serving() -> Result<()> {
    init_tracing();

    let (gw_bin, rt_bin) = match bins_or_skip() {
        Some(b) => b,
        None => {
            eprintln!("[ac5] SKIP: gateway/runtime bins not built; run `cargo build -p sera-gateway -p sera-runtime` first");
            return Ok(());
        }
    };
    let (llm, _mock) = match start_mock_llm().await {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[ac5] SKIP: wiremock unavailable ({e})");
            return Ok(());
        }
    };

    // Per-test admin socket so concurrent test runs don't collide on the
    // default `/var/lib/sera/admin.sock` / `/tmp/sera-admin-$USER.sock`.
    let sock_dir = tempfile::tempdir().context("creating admin sock tempdir")?;
    let sock_path = sock_dir.path().join("admin.sock");
    let sock_str = sock_path
        .to_str()
        .context("admin sock path is not UTF-8")?
        .to_owned();

    // We rely on the harness default for `SERA_ADMIN_TOKEN` (admin HTTP is
    // not exercised here — the kill-switch flow goes over the unix socket).
    let gw = InProcessGateway::start_local_with_env(
        &gw_bin,
        &rt_bin,
        &llm,
        &[("SERA_ADMIN_SOCK", sock_str.as_str())],
    )
    .await
    .context("booting gateway for ac5 kill-switch rollback")?;

    let gateway_pid = gw.child_pid().context("gateway child has no PID")?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("building HTTP client")?;

    // Drive a turn end-to-end so a `sera-runtime` child is parented to the
    // gateway. The mock LLM replies near-instantly, so the chat round-trip
    // completes; the runtime child stays around for subsequent turns.
    let chat = http
        .post(format!("{}/api/chat", gw.base_url))
        .json(&json!({
            "agent": "sera",
            "message": "ac5: spawn runtime",
            "stream": false,
        }))
        .send()
        .await
        .context("first /api/chat send")?;
    anyhow::ensure!(
        chat.status().is_success(),
        "first /api/chat must succeed before rollback; got {}",
        chat.status()
    );

    // Wait briefly for the runtime child to register in /proc.
    let mut pre_rollback_pids: Vec<u32> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        pre_rollback_pids = collect_child_pids(gateway_pid);
        if !pre_rollback_pids.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    anyhow::ensure!(
        !pre_rollback_pids.is_empty(),
        "expected at least one runtime child of gateway {gateway_pid} before rollback; \
         saw none — kill switch rollback evidence requires a live target"
    );
    eprintln!("[ac5] pre-rollback runtime children of gateway {gateway_pid}: {pre_rollback_pids:?}");

    // Connect with retry: the listener binds in a tokio task that may race
    // the first /api/health probe.
    let mut stream = None;
    for _ in 0..30 {
        match UnixStream::connect(&sock_path).await {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    let mut stream = stream.context("admin socket did not come up within 3s of boot")?;

    // The admin sock awaits the rollback callback before replying with
    // `OK\n` (sera-40y3). Reading the response is the synchronisation
    // point: by the time we have it, every `kill_for_rollback` has run and
    // the killed runtime child has been reaped via `child.wait()`.
    stream
        .write_all(b"ROLLBACK\n")
        .await
        .context("writing ROLLBACK to admin socket")?;
    let mut response = String::new();
    timeout(Duration::from_secs(15), stream.read_to_string(&mut response))
        .await
        .context("admin socket ROLLBACK reply timed out")?
        .context("reading ROLLBACK reply")?;
    anyhow::ensure!(
        response.contains("OK") || response.contains("ARMED"),
        "expected admin socket to ack ROLLBACK with OK/ARMED, got {response:?}"
    );

    // ── AC5 evidence #1: the pre-rollback runtime PIDs are gone ──
    //
    // We poll briefly because /proc updates lag SIGKILL by a tick or two on
    // loaded machines.
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut still_alive: Vec<u32>;
    loop {
        still_alive = Vec::new();
        for &pid in &pre_rollback_pids {
            if pid_alive(pid) {
                still_alive.push(pid);
            }
        }
        if still_alive.is_empty() {
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    anyhow::ensure!(
        still_alive.is_empty(),
        "expected pre-rollback runtime PIDs {pre_rollback_pids:?} to be reaped within 3s of \
         ROLLBACK; still alive: {still_alive:?}"
    );
    eprintln!("[ac5] pre-rollback PIDs reaped — AC5 stop-the-lane evidence OK");

    // ── AC5 evidence #2: gateway itself stayed up ──
    anyhow::ensure!(
        pid_alive(gateway_pid),
        "gateway pid {gateway_pid} died after ROLLBACK; AC5 forbids gateway corruption"
    );

    // ── DISARM and confirm a fresh submission is accepted ──
    let mut disarm_stream = UnixStream::connect(&sock_path)
        .await
        .context("reconnecting admin socket for DISARM")?;
    disarm_stream
        .write_all(b"DISARM\n")
        .await
        .context("writing DISARM")?;
    let mut disarm_resp = String::new();
    timeout(Duration::from_secs(2), disarm_stream.read_to_string(&mut disarm_resp))
        .await
        .context("admin socket DISARM reply timed out")?
        .context("reading DISARM reply")?;
    anyhow::ensure!(
        disarm_resp.contains("OK"),
        "expected DISARM ack OK, got {disarm_resp:?}"
    );

    // AC5 evidence #3: post-DISARM submission accepted within 5s.
    let post = timeout(Duration::from_secs(5), async {
        http.post(format!("{}/api/chat", gw.base_url))
            .json(&json!({
                "agent": "sera",
                "message": "ac5: post-rollback resume",
                "stream": false,
            }))
            .send()
            .await
            .and_then(|r| r.error_for_status())
    })
    .await
    .context("post-DISARM /api/chat did not return within 5s")?;
    let post = post.context("post-DISARM /api/chat returned non-2xx")?;
    eprintln!(
        "[ac5] post-DISARM /api/chat status: {} — resume evidence OK",
        post.status()
    );

    if let Err(e) = gw.shutdown().await {
        tracing::warn!(error = %e, "gateway shutdown returned error (non-fatal)");
    }

    Ok(())
}
