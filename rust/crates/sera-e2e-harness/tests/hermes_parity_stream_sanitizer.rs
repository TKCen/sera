//! Hermes parity matrix Row 4 — **streaming** sanitizer regression test.
//!
//! The companion non-streaming test (`hermes_parity_response_sanitizer.rs`)
//! proves that a complete `<think>…</think>` block in the final reply is
//! stripped before the JSON `response` field is returned.  This test proves
//! the **streaming** path:
//!
//! - The mock LLM sends the `<think>` tag split across multiple SSE delta
//!   chunks so no single chunk contains a complete, easy-to-detect block.
//! - The gateway's `StreamThinkSanitizer` must correctly carry state between
//!   chunks and suppress all think-block content in every forwarded delta.
//! - The final assistant transcript stored in SQLite must also be sanitized
//!   (no raw chain-of-thought).
//!
//! ## Skip contract
//!
//! Missing binaries or a wiremock bind failure print a single skip line to
//! stderr and return `Ok(())` rather than failing — matches the pattern used
//! by all other integration tests in this crate.  Streaming mode is always
//! tested with the scripted mock LLM; if `SERA_E2E_LLM_BASE_URL` is set we
//! skip rather than sending an unpredictable request to a live model.

#![cfg(feature = "integration")]

use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde_json::json;

use sera_cli::sse::{SseClient, StreamEvent};
use sera_e2e_harness::binaries::{gateway_bin, runtime_bin};
use sera_e2e_harness::mock_llm::start_mock_llm_with_chunks;
use sera_e2e_harness::InProcessGateway;

const SKIP_TAG: &str = "[hermes-parity-stream-sanitizer]";

/// The mock LLM reply — `<think>…</think>` block split across three chunks so
/// no individual SSE delta contains a detectable complete tag on its own.
///
/// Chunk layout:
///   chunk 0: `<thi`            — opens the think tag but leaves it incomplete
///   chunk 1: `nk>hidden </`    — completes `<think>`, starts closing tag
///   chunk 2: `think>OK `       — completes `</think>` and begins visible reply
///   chunk 3: `<nonce>`         — visible reply continued (carries the nonce)
///
/// The gateway must reconstruct the full tag boundary across chunks and emit
/// only `"OK <nonce>"` to the SSE client, with no `<think>` or `</think>`
/// leaking in any delta.
fn split_think_chunks(nonce: &str) -> Vec<String> {
    vec![
        "<thi".to_owned(),
        "nk>hidden </".to_owned(),
        "think>OK ".to_owned(),
        nonce.to_owned(),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hermes_parity_stream_sanitizer_no_think_in_any_delta() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SERA_E2E_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_test_writer()
        .try_init();

    // ── 0. Skip when operator LLM is set (scripted mock required) ──
    if std::env::var("SERA_E2E_LLM_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_some()
    {
        eprintln!(
            "{SKIP_TAG} SKIP: SERA_E2E_LLM_BASE_URL is set; this test \
             requires the scripted mock LLM for deterministic split-chunk output."
        );
        return Ok(());
    }

    // ── 1. Locate binaries ──
    let gateway_bin_path = match gateway_bin() {
        Some(p) => p,
        None => {
            eprintln!(
                "{SKIP_TAG} SKIP: sera-gateway binary not found. \
                 Run `cargo build -p sera-gateway` first."
            );
            return Ok(());
        }
    };
    let runtime_bin_path = match runtime_bin() {
        Some(p) => p,
        None => {
            eprintln!(
                "{SKIP_TAG} SKIP: sera-runtime binary not found. \
                 Run `cargo build -p sera-runtime` first."
            );
            return Ok(());
        }
    };

    // ── 2. Start mock LLM with split-chunk think reply ──
    let nonce = short_nonce();
    let chunks = split_think_chunks(&nonce);
    let chunk_strs: Vec<&str> = chunks.iter().map(String::as_str).collect();

    let (llm_base_url, _mock_handle) = match start_mock_llm_with_chunks(&chunk_strs).await {
        Ok((url, server)) => (url, server),
        Err(e) => {
            eprintln!(
                "{SKIP_TAG} SKIP: could not start local mock LLM ({e}). \
                 Streaming sanitizer regression needs a wiremock bind."
            );
            return Ok(());
        }
    };

    // ── 3. Boot gateway ──
    let gateway = InProcessGateway::start_local(
        &gateway_bin_path,
        &runtime_bin_path,
        &llm_base_url,
    )
    .await
    .context("gateway failed to boot — streaming sanitizer regression")?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("building streaming sanitizer HTTP client")?;

    let bearer_token = std::env::var("SERA_E2E_BEARER_TOKEN")
        .unwrap_or_else(|_| "sera_bootstrap_dev_123".to_string());

    // ── 4. Make streaming /api/chat request ──
    let sse = SseClient::new(http, gateway.base_url.clone());
    let mut stream = sse
        .post_stream(
            "/api/chat",
            json!({
                "agent": "sera",
                "message": format!("Reply OK followed by the nonce {nonce}."),
                "stream": true,
            }),
        )
        .await
        .context("opening SSE stream for streaming sanitizer test")?;

    // ── 5. Drain all SSE events ──
    let mut all_deltas: Vec<String> = Vec::new();
    let mut session_id = String::new();
    let mut saw_done = false;

    let budget = tokio::time::Instant::now() + Duration::from_secs(25);
    loop {
        let tick = tokio::time::timeout_at(budget, stream.next()).await;
        let ev = match tick {
            Err(_) => anyhow::bail!(
                "{SKIP_TAG} SSE stream timed out — \
                 saw {} deltas, done={saw_done}",
                all_deltas.len()
            ),
            Ok(None) => break,
            Ok(Some(Err(e))) => anyhow::bail!("SSE parse error: {e}"),
            Ok(Some(Ok(ev))) => ev,
        };

        match ev {
            StreamEvent::Token { delta, session_id: sid } => {
                if session_id.is_empty() {
                    session_id = sid;
                }
                all_deltas.push(delta);
            }
            StreamEvent::Done { .. } => {
                saw_done = true;
                break;
            }
            StreamEvent::Error { message } => {
                anyhow::bail!(
                    "{SKIP_TAG} SSE stream returned error event: {message}"
                );
            }
            _ => {}
        }
    }

    // ── 6. Assert: no think markers in any delta ──
    for (i, delta) in all_deltas.iter().enumerate() {
        let lc = delta.to_ascii_lowercase();
        assert!(
            !lc.contains("<think>"),
            "{SKIP_TAG} FAIL: streaming delta #{i} leaked `<think>` marker: {delta:?}"
        );
        assert!(
            !lc.contains("</think>"),
            "{SKIP_TAG} FAIL: streaming delta #{i} leaked `</think>` marker: {delta:?}"
        );
        assert!(
            !lc.contains("hidden"),
            "{SKIP_TAG} FAIL: streaming delta #{i} leaked hidden chain-of-thought: {delta:?}"
        );
    }

    // ── 7. Assert: at least one delta and the stream closed cleanly ──
    assert!(
        !all_deltas.is_empty(),
        "{SKIP_TAG} FAIL: received zero SSE message events — \
         sanitizer may have over-trimmed the visible reply"
    );
    assert!(saw_done, "{SKIP_TAG} FAIL: SSE stream did not close with a `done` event");

    // ── 8. Assert: combined delta text contains the nonce ──
    let combined: String = all_deltas.concat();
    assert!(
        combined.contains(&nonce),
        "{SKIP_TAG} FAIL: combined streaming output missing nonce {nonce:?}. \
         Got: {combined:?}"
    );

    // ── 9. Assert: stored transcript is sanitized ──
    if !session_id.is_empty() && gateway.db_path.exists() {
        let conn = rusqlite::Connection::open_with_flags(
            &gateway.db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .context("opening SQLite for transcript assertion")?;

        let transcript: Option<String> = conn
            .query_row(
                "SELECT content FROM transcript \
                 WHERE session_id = ?1 AND role = 'assistant' \
                 ORDER BY rowid DESC LIMIT 1",
                rusqlite::params![session_id],
                |row| row.get(0),
            )
            .ok();

        if let Some(text) = transcript {
            let lc = text.to_ascii_lowercase();
            assert!(
                !lc.contains("<think>"),
                "{SKIP_TAG} FAIL: stored transcript leaked `<think>`: {text:?}"
            );
            assert!(
                !lc.contains("</think>"),
                "{SKIP_TAG} FAIL: stored transcript leaked `</think>`: {text:?}"
            );
            assert!(
                !lc.contains("hidden"),
                "{SKIP_TAG} FAIL: stored transcript leaked chain-of-thought content: {text:?}"
            );
        }
    }

    gateway
        .shutdown()
        .await
        .context("graceful gateway shutdown")?;
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
