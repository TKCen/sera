//! S1 — Bootstrap & health scenarios (Phase 1 of the TEST-SCENARIOS plan).
//!
//! These tests prove the zero-to-hello operator path: a fresh checkout can
//! boot the gateway against a minimal manifest and respond on the expected
//! health surfaces.  They reuse the [`InProcessGateway`] harness and its
//! wiremock-backed mock LLM so a bare CI environment can run them without
//! reaching the network.
//!
//! Scope intentionally excludes the `sera start --local` code path: that
//! command hard-probes `http://localhost:1234/v1/models` and fails fast if
//! unreachable.  Tests against it would need a port-1234 mock (which races
//! with anything else on the dev box) or a probe-URL env override — that's
//! a product change filed as a follow-up bead, not Phase 1 scope.
//!
//! Covered:
//! * S1.1 — gateway boots + `/api/health` + `/api/health/ready` + `/api/auth/me`
//!   all answer successfully after a single `--config --port` spawn.
//! * S1.2 — SQLite state persists across a restart: post a turn, shut down,
//!   boot again against the same root, and verify the old session's
//!   transcript rows are still readable through `/api/sessions/.../transcript`.

#![cfg(feature = "integration")]

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;

use sera_e2e_harness::binaries::{gateway_bin, runtime_bin};
use sera_e2e_harness::mock_llm::start_mock_llm;
use sera_e2e_harness::{count_transcript_rows, GatewayRoot, InProcessGateway};

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SERA_E2E_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_test_writer()
        .try_init();
}

/// Skip if the gateway/runtime bins aren't built.  Returns the two paths as
/// a tuple so the caller can thread them into the harness in one shot.
fn bins_or_skip() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let gw = gateway_bin()?;
    let rt = runtime_bin()?;
    Some((gw, rt))
}

/// S1.1 — After a `sera-gateway start --config X --port Y` spawn against a
/// fresh tempdir, the three health surfaces all answer 200.  Proves that the
/// operator onboarding path (build binaries → write `sera.yaml` → start) is
/// unbroken end-to-end.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s1_1_bootstrap_binds_and_health_responds() -> Result<()> {
    init_tracing();

    let (gw_bin, rt_bin) = match bins_or_skip() {
        Some(b) => b,
        None => {
            eprintln!("[S1.1] SKIP: gateway/runtime bins not built");
            return Ok(());
        }
    };
    let (llm, _mock) = match start_mock_llm().await {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[S1.1] SKIP: wiremock unavailable ({e})");
            return Ok(());
        }
    };

    let gateway = InProcessGateway::start_local(&gw_bin, &rt_bin, &llm)
        .await
        .context("booting gateway for S1.1")?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    // /api/health — liveness
    let h: reqwest::Response = http
        .get(format!("{}/api/health", gateway.base_url))
        .send()
        .await?;
    assert!(h.status().is_success(), "/api/health expected 2xx, got {}", h.status());

    // /api/health/ready — readiness (must land after boot)
    let r: reqwest::Response = http
        .get(format!("{}/api/health/ready", gateway.base_url))
        .send()
        .await?;
    assert!(
        r.status().is_success(),
        "/api/health/ready expected 2xx once boot completes, got {}",
        r.status()
    );

    // /api/auth/me — principal surface; autonomous mode returns a stable sub
    let me: serde_json::Value = http
        .get(format!("{}/api/auth/me", gateway.base_url))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(
        me.get("sub").and_then(|v| v.as_str()).is_some(),
        "auth/me must include `sub`, got {me:?}"
    );

    if let Err(e) = gateway.shutdown().await {
        eprintln!("gateway shutdown returned: {e}");
    }
    Ok(())
}

/// S1.2 — SQLite-backed session state survives a restart.  Post a turn under
/// one gateway instance, shut it down, boot a second gateway against the
/// same [`GatewayRoot`] (same tempdir, same `sera.db`), and prove the old
/// session's transcript rows still exist both on disk and via the HTTP
/// transcript surface.
///
/// This is the honest end-to-end assertion that `sera-local`'s "your data
/// lives in ./sera-local/" promise is real — a crash at any point between
/// the two boots doesn't lose the user's history.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s1_2_restart_preserves_session_state() -> Result<()> {
    init_tracing();

    let (gw_bin, rt_bin) = match bins_or_skip() {
        Some(b) => b,
        None => {
            eprintln!("[S1.2] SKIP: gateway/runtime bins not built");
            return Ok(());
        }
    };
    let (llm, _mock) = match start_mock_llm().await {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[S1.2] SKIP: wiremock unavailable ({e})");
            return Ok(());
        }
    };

    // Caller-owned root — outlives both gateway handles.
    let model = sera_e2e_harness::resolve_model_env();
    let root = GatewayRoot::new_local(&llm, &model).context("creating shared root")?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    // ── Boot 1: create a session by running a turn ──
    let session_id_before = {
        let gw = InProcessGateway::start_with_root(&root, &gw_bin, &rt_bin, &llm)
            .await
            .context("first boot")?;
        let resp: serde_json::Value = http
            .post(format!("{}/api/chat", gw.base_url))
            .json(&json!({
                "agent": "sera",
                "message": "persist me across a restart",
                "stream": false,
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let id = resp
            .get("session_id")
            .and_then(|v| v.as_str())
            .context("first turn must return session_id")?
            .to_owned();
        if let Err(e) = gw.shutdown().await {
            eprintln!("gateway shutdown returned: {e}");
        }
        id
    };

    // ── Boot 2: same root — must surface the prior session ──
    let gw2 = InProcessGateway::start_with_root(&root, &gw_bin, &rt_bin, &llm)
        .await
        .context("second boot against shared root")?;

    // HTTP view: transcript endpoint must still know the old session.
    let transcript: serde_json::Value = http
        .get(format!("{}/api/sessions/{}/transcript", gw2.base_url, session_id_before))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let entries = transcript
        .as_array()
        .expect("transcript must be a JSON array");
    let user_turns = entries
        .iter()
        .filter(|e| e.get("role").and_then(|r| r.as_str()) == Some("user"))
        .count();
    assert!(
        user_turns >= 1,
        "after restart the user turn must still be visible via transcript endpoint, got {entries:?}"
    );

    // Direct SQLite view — belt + braces: if the HTTP view added a cache in
    // between, this still proves the rows are durable.
    let sql_user = count_transcript_rows(&root.db_path, &session_id_before, "user")?;
    assert!(sql_user >= 1, "sqlite transcript must retain user row after restart");

    if let Err(e) = gw2.shutdown().await {
        eprintln!("gateway shutdown returned: {e}");
    }
    Ok(())
}
