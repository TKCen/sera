//! S5 — HITL scenarios.
//!
//! Wave D Phase 1 shipped `GET /api/hitl/requests` + approve/reject/escalate
//! routes, backed by the `sera-hitl` ticket store.  This file exercises the
//! HTTP surface end-to-end; the full approve-resumes-turn flow (S5.2+) is
//! tracked separately because it needs a runtime path that actually raises
//! a HITL ticket (enforcement_mode: strict + a tool dispatch that the
//! ApprovalRouter flags).
//!
//! Covered:
//! * S5.0 — `GET /api/hitl/requests` on a freshly-booted gateway returns
//!   the documented `{tickets: [], count: 0}` shape.  Regression guard for
//!   the route's JSON response contract.
//! * S5.3 — approving / rejecting / escalating a non-existent ticket id
//!   returns `404 Not Found`.  Guards the error path: a malicious or
//!   confused client can't fabricate approvals for tickets that don't
//!   exist.

#![cfg(feature = "integration")]

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

/// S5.0 — a fresh gateway's HITL queue is empty and the list endpoint
/// returns the `{tickets: [], count: 0}` shape documented in Wave D.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s5_0_hitl_queue_is_empty_on_fresh_gateway() -> Result<()> {
    init_tracing();

    let (gw_bin, rt_bin) = match bins_or_skip() {
        Some(b) => b,
        None => {
            eprintln!("[S5.0] SKIP: gateway/runtime bins not built");
            return Ok(());
        }
    };
    let (llm, _mock) = match start_mock_llm().await {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[S5.0] SKIP: wiremock unavailable ({e})");
            return Ok(());
        }
    };

    let gw = InProcessGateway::start_local(&gw_bin, &rt_bin, &llm)
        .await
        .context("booting gateway for S5.0")?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let list: serde_json::Value = http
        .get(format!("{}/api/hitl/requests", gw.base_url))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let tickets = list
        .get("tickets")
        .and_then(|v| v.as_array())
        .context("response missing `tickets` array")?;
    let count = list
        .get("count")
        .and_then(|v| v.as_u64())
        .context("response missing `count`")?;
    assert_eq!(tickets.len(), 0, "fresh gateway must have no tickets, got {tickets:?}");
    assert_eq!(count, 0, "count must agree with tickets.len()");

    if let Err(e) = gw.shutdown().await {
        eprintln!("gateway shutdown returned: {e}");
    }
    Ok(())
}

/// S5.3 — approve / reject / escalate on an unknown ticket id returns 404.
/// Guards the error path: a client (malicious or confused) cannot fabricate
/// an approval for a ticket that doesn't exist.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s5_3_hitl_actions_on_missing_ticket_return_404() -> Result<()> {
    init_tracing();

    let (gw_bin, rt_bin) = match bins_or_skip() {
        Some(b) => b,
        None => {
            eprintln!("[S5.3] SKIP: gateway/runtime bins not built");
            return Ok(());
        }
    };
    let (llm, _mock) = match start_mock_llm().await {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[S5.3] SKIP: wiremock unavailable ({e})");
            return Ok(());
        }
    };

    let gw = InProcessGateway::start_local(&gw_bin, &rt_bin, &llm)
        .await
        .context("booting gateway for S5.3")?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    for action in ["approve", "reject", "escalate"] {
        let resp = http
            .post(format!(
                "{}/api/hitl/requests/tkt_does_not_exist/{}",
                gw.base_url, action
            ))
            .json(&serde_json::json!({}))
            .send()
            .await?;
        assert_eq!(
            resp.status(),
            reqwest::StatusCode::NOT_FOUND,
            "{action} on missing ticket should be 404; got {}",
            resp.status()
        );
    }

    if let Err(e) = gw.shutdown().await {
        eprintln!("gateway shutdown returned: {e}");
    }
    Ok(())
}
