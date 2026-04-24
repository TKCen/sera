//! S7 — Workflow / task planning scenarios (Phase 3 of the TEST-SCENARIOS
//! plan).
//!
//! Covers the workflow scheduler's HTTP surface.  Phase 1 of Wave E shipped
//! only the Timer gate; other `await_type` values return 501 and are tracked
//! as follow-up beads.  This file asserts the happy path end-to-end: POST a
//! timer task with a near-future deadline, poll until the 5-second scheduler
//! tick flips its status to `resolved`, and verify the `resolved_at`
//! timestamp is populated.
//!
//! Covered:
//! * S7.1 — Timer task posted with a 1-second deadline transitions
//!   `pending → resolved` after the next scheduler tick (≤ ~8s wall-clock).
//! * S7.2 — `await_type: human` returns `501 Not Implemented` — pins the
//!   current Phase 1 scope so a silent "it works!" regression on a partial
//!   wiring can't slip through.

#![cfg(feature = "integration")]

use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{Duration as ChronoDuration, Utc};

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

/// S7.1 — a Timer task flows from `pending` to `resolved` after the
/// scheduler's next tick elapses past the deadline.
///
/// The scheduler ticks every 5s (hard-coded in `sera_gateway::scheduler`).
/// To keep the wall-clock budget predictable we set the deadline 1s in the
/// future and allow up to 10s of polling — one tick on the far side of the
/// deadline is guaranteed within that window.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s7_1_timer_task_resolves_after_scheduler_tick() -> Result<()> {
    init_tracing();

    let (gw_bin, rt_bin) = match bins_or_skip() {
        Some(b) => b,
        None => {
            eprintln!("[S7.1] SKIP: gateway/runtime bins not built");
            return Ok(());
        }
    };
    let (llm, _mock) = match start_mock_llm().await {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[S7.1] SKIP: wiremock unavailable ({e})");
            return Ok(());
        }
    };

    let gw = InProcessGateway::start_local(&gw_bin, &rt_bin, &llm)
        .await
        .context("booting gateway for S7.1")?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    // Deadline is 1s in the future — the scheduler's 5s tick will observe
    // it as elapsed on one of the next two ticks.
    let deadline = Utc::now() + ChronoDuration::seconds(1);
    let body = serde_json::json!({
        "await_type": "timer",
        "agent_id": "sera",
        "resume_token": "s7-1-token",
        "deadline": deadline.to_rfc3339(),
        "title": "S7.1 smoketest",
    });

    let created: serde_json::Value = http
        .post(format!("{}/api/workflow/tasks", gw.base_url))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let task_id = created
        .get("id")
        .and_then(|v| v.as_str())
        .context("create response missing id")?
        .to_owned();
    assert_eq!(
        created.get("status").and_then(|v| v.as_str()),
        Some("pending"),
        "freshly-created timer task must be pending: {created:?}"
    );

    // Poll the single-task endpoint until status flips.  Upper bound is
    // 10s (~two scheduler ticks of 5s each) — if it hasn't resolved by
    // then something is wrong with the scheduler.
    let mut final_status: Option<String> = None;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        let view: serde_json::Value = http
            .get(format!("{}/api/workflow/tasks/{}", gw.base_url, task_id))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let status = view
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        if status == "resolved" {
            assert!(
                view.get("resolved_at")
                    .and_then(|v| v.as_str())
                    .is_some(),
                "resolved task must carry a resolved_at timestamp: {view:?}"
            );
            final_status = Some(status);
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert_eq!(
        final_status.as_deref(),
        Some("resolved"),
        "timer task did not resolve within 10s; last poll after {:?}",
        start.elapsed()
    );

    if let Err(e) = gw.shutdown().await {
        eprintln!("gateway shutdown returned: {e}");
    }
    Ok(())
}

/// S7.2 — non-Timer `await_type` values return 501 Not Implemented.
///
/// Pins the current Wave E Phase 1 scope: only `timer` is wired.  When the
/// remaining gates land (Human, GhRun, GhPr, Change, Mail — filed as
/// sera-dgk1 et al.) this test will start failing with 200 OK, and the
/// author of the landing PR is expected to update it to an assertion
/// appropriate for that gate.  Failing loudly is the point.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s7_2_non_timer_await_type_returns_501() -> Result<()> {
    init_tracing();

    let (gw_bin, rt_bin) = match bins_or_skip() {
        Some(b) => b,
        None => {
            eprintln!("[S7.2] SKIP: gateway/runtime bins not built");
            return Ok(());
        }
    };
    let (llm, _mock) = match start_mock_llm().await {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[S7.2] SKIP: wiremock unavailable ({e})");
            return Ok(());
        }
    };

    let gw = InProcessGateway::start_local(&gw_bin, &rt_bin, &llm)
        .await
        .context("booting gateway for S7.2")?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let body = serde_json::json!({
        "await_type": "human",
        "agent_id": "sera",
        "resume_token": "s7-2-token",
        "title": "S7.2 human-gate probe",
    });
    let resp = http
        .post(format!("{}/api/workflow/tasks", gw.base_url))
        .json(&body)
        .send()
        .await?;
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::NOT_IMPLEMENTED,
        "non-Timer await_type should be 501 until that gate ships"
    );

    if let Err(e) = gw.shutdown().await {
        eprintln!("gateway shutdown returned: {e}");
    }
    Ok(())
}
