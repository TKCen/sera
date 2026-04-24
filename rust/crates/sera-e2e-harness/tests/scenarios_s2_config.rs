//! S2 — Config & manifest scenarios (Phase 2 of the TEST-SCENARIOS plan).
//!
//! Covers the gateway's manifest-loading surface — operators author a
//! `sera.yaml` on disk; the gateway reads, validates, and exposes its view
//! via `/api/agents`.  If this surface regresses every downstream feature
//! depends on undefined behaviour, so a minimal load+list scenario is the
//! cheapest regression guard.
//!
//! Covered:
//! * S2.1 — a manifest declaring two agents produces a `/api/agents`
//!   response listing both by name.  Follow-ups (hot-reload, secret
//!   resolution, policy_ref binding) are filed separately so this file
//!   stays focused on the happy path.

#![cfg(feature = "integration")]

use std::time::Duration;

use anyhow::{Context, Result};

use sera_e2e_harness::binaries::{gateway_bin, runtime_bin};
use sera_e2e_harness::mock_llm::start_mock_llm;
use sera_e2e_harness::{multi_agent_sera_yaml, resolve_model_env, GatewayRoot, InProcessGateway};

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

/// S2.1 — A two-agent manifest loads and `GET /api/agents` lists both by
/// name.  Guards against regressions in the manifest loader's multi-document
/// YAML parsing and the gateway's `agents_handler` response shape.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s2_1_multi_agent_manifest_loads_and_lists() -> Result<()> {
    init_tracing();

    let (gw_bin, rt_bin) = match bins_or_skip() {
        Some(b) => b,
        None => {
            eprintln!("[S2.1] SKIP: gateway/runtime bins not built");
            return Ok(());
        }
    };
    let (llm, _mock) = match start_mock_llm().await {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[S2.1] SKIP: wiremock unavailable ({e})");
            return Ok(());
        }
    };

    let model = resolve_model_env();
    let manifest = multi_agent_sera_yaml(&llm, &model, &["sera", "helper"]);
    let root = GatewayRoot::new_with_manifest(&manifest).context("building multi-agent root")?;

    let gw = InProcessGateway::start_with_root(&root, &gw_bin, &rt_bin, &llm)
        .await
        .context("booting gateway for S2.1")?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let agents: serde_json::Value = http
        .get(format!("{}/api/agents", gw.base_url))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let arr = agents.as_array().context("agents must be a JSON array")?;
    assert_eq!(arr.len(), 2, "expected exactly two agents, got {arr:?}");

    // Collect names into a set — the gateway is free to order the list.
    let names: std::collections::HashSet<&str> = arr
        .iter()
        .filter_map(|a| a.get("name").and_then(|v| v.as_str()))
        .collect();
    assert!(
        names.contains("sera") && names.contains("helper"),
        "expected agents `sera` and `helper`, got {names:?}"
    );

    if let Err(e) = gw.shutdown().await {
        eprintln!("gateway shutdown returned: {e}");
    }
    Ok(())
}
