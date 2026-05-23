//! Hermes parity matrix Row 4 — response sanitizer regression test.
//!
//! Companion to `hermes_parity_baseline.rs`. The baseline gate asserts that
//! a *typical* mock-LLM reply never carries `<think>` markers; this test
//! goes one step further and proves the assertion for a reasoning-model
//! style reply that *does* contain `<think>…</think>` blocks. The mock LLM
//! emits chain-of-thought text deliberately; the gateway must strip it
//! before the operator-visible `response` is returned.
//!
//! Without the gateway-side sanitizer this test fails — the mock's raw
//! `<think>` block bleeds straight into `ChatResponse.response`. With the
//! sanitizer applied (`sera_gateway::response_sanitizer`) the operator
//! sees only the visible reply.
//!
//! Skip contract matches the baseline gate: missing binaries or an
//! unavailable mock-LLM bind print a single skip line and return `Ok(())`.

#![cfg(feature = "integration")]

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;

use sera_e2e_harness::binaries::{gateway_bin, runtime_bin};
use sera_e2e_harness::mock_llm::start_mock_llm_with_reply;
use sera_e2e_harness::InProcessGateway;

const SKIP_TAG: &str = "[hermes-parity-response-sanitizer]";

/// Reply the mock LLM serves — a balanced `<think>` block followed by the
/// visible reply. The visible portion embeds a per-test nonce so the
/// assertions can pin down "the model said exactly this" rather than
/// matching loosely.
fn mock_reply_with_think(nonce: &str) -> String {
    format!(
        "<think>I should answer briefly with the nonce {nonce}.\nLet me not leak this reasoning.</think>OK {nonce}"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hermes_parity_response_sanitizer_strips_think_blocks() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SERA_E2E_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_test_writer()
        .try_init();

    // ── 0. Binaries ──
    let gateway_bin_path = match gateway_bin() {
        Some(p) => p,
        None => {
            eprintln!(
                "{SKIP_TAG} SKIP: sera-gateway binary not found next to the \
                 test binary. Run `cargo build -p sera-gateway` first."
            );
            return Ok(());
        }
    };
    let runtime_bin_path = match runtime_bin() {
        Some(p) => p,
        None => {
            eprintln!(
                "{SKIP_TAG} SKIP: sera-runtime binary not found next to the \
                 test binary. Run `cargo build -p sera-runtime` first."
            );
            return Ok(());
        }
    };

    let nonce = short_nonce();
    let mock_reply = mock_reply_with_think(&nonce);

    // Unlike the baseline gate, this test *requires* the scripted mock
    // LLM. A real upstream LLM has no reliable way to be coerced into
    // emitting `<think>` markers verbatim across providers, so when the
    // operator overrides `SERA_E2E_LLM_BASE_URL` we skip with a clear
    // line rather than running a flaky live capture.
    if std::env::var("SERA_E2E_LLM_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_some()
    {
        eprintln!(
            "{SKIP_TAG} SKIP: SERA_E2E_LLM_BASE_URL is set; this test \
             requires the scripted mock LLM so a `<think>` block can be \
             produced deterministically."
        );
        return Ok(());
    }

    let (llm_base_url, _mock_handle) = match start_mock_llm_with_reply(&mock_reply).await {
        Ok((url, server)) => (url, server),
        Err(e) => {
            eprintln!(
                "{SKIP_TAG} SKIP: could not start local mock LLM ({e}). \
                 Sanitizer regression needs a wiremock bind."
            );
            return Ok(());
        }
    };

    let gateway = InProcessGateway::start_local(
        &gateway_bin_path,
        &runtime_bin_path,
        &llm_base_url,
    )
    .await
    .context("gateway failed to boot — sanitizer regression")?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("building sanitizer regression HTTP client")?;

    let bearer_token = std::env::var("SERA_E2E_BEARER_TOKEN")
        .unwrap_or_else(|_| "sera_bootstrap_dev_123".to_string());
    let chat: serde_json::Value = http
        .post(format!("{}/api/chat", gateway.base_url))
        .bearer_auth(&bearer_token)
        .json(&json!({
            "agent": "sera",
            "message": format!("Reply with the word OK and the nonce {nonce}."),
            "stream": false,
        }))
        .send()
        .await
        .context("sanitizer probe send")?
        .error_for_status()
        .context("sanitizer probe HTTP status")?
        .json()
        .await
        .context("sanitizer probe JSON parse")?;

    let response_text = chat
        .get("response")
        .and_then(|v| v.as_str())
        .with_context(|| format!("sanitizer probe missing response: {chat:?}"))?
        .to_owned();

    // Hard contract: regardless of what the model said inside the
    // `<think>` block, the operator-visible response must not contain
    // the markers themselves nor the reasoning text we embedded.
    let lc = response_text.to_ascii_lowercase();
    assert!(
        !lc.contains("<think>"),
        "sanitizer regression: operator-visible response leaked `<think>` \
         marker: {response_text:?}"
    );
    assert!(
        !lc.contains("</think>"),
        "sanitizer regression: operator-visible response leaked `</think>` \
         marker: {response_text:?}"
    );
    assert!(
        !lc.contains("leak this reasoning"),
        "sanitizer regression: hidden reasoning text leaked into \
         operator-visible response: {response_text:?}"
    );

    // The visible portion must survive — the sanitizer must not over-trim.
    assert!(
        response_text.contains(&nonce),
        "sanitizer regression: operator-visible response lost the nonce \
         {nonce:?} that lived outside the `<think>` block: \
         {response_text:?}"
    );

    gateway.shutdown().await.context("graceful gateway shutdown")?;
    Ok(())
}

fn short_nonce() -> String {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{pid:x}{nanos:x}")
}
