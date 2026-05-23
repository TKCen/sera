//! Hermes parity matrix Row 1 — `/api/chat` negative + isolation smoke.
//!
//! Companion to `hermes_parity_baseline.rs` (positive baseline) and
//! `hermes_parity_response_sanitizer.rs` (Row 4 regression). The baseline gate
//! proves the happy path; this file extends the smoke to the two contracts
//! the Kanban Discord/API parity slice explicitly named as visible-regression
//! gaps:
//!
//! 1. **Lane-busy negative path** — two concurrent `POST /api/chat` requests
//!    for the same session-key. The gateway must admit one and short-circuit
//!    the other with HTTP 429, `Retry-After: 15`, and a structured
//!    `{error: "rate_limited", reason: "lane_busy", retry_after_secs: 15}`
//!    body. The probe asserts "exactly one 429 and one 2xx" rather than
//!    binding to physical request order, so a contended CI worker that
//!    flips the lane-mutex race does not flake the test. This is the
//!    operator-visible "blocked" state per the acceptance criterion;
//!    without this gate, a flooded client either wedges the lane or gets
//!    opaque 500s, both of which look like "thinking forever" to the
//!    operator.
//!
//! 2. **Session isolation across agents** — when the manifest declares two
//!    agents and operator turns are sent to each, the gateway must hand out
//!    distinct `session_id`s and the SQLite transcript must contain each
//!    user's message *only* in its own session. This pins down the matrix
//!    Row 1 claim that `/api/chat` is "session-scoped through gateway-owned
//!    SQLite".
//!
//! Skip contract matches the baseline gate: missing binaries or an
//! unavailable mock-LLM bind print a single skip line and return `Ok(())`.
//!
//! Source beads: `sera-duo3` (CI smoke), `sera-yj18` (streaming/edge cases).
//! Tied to the parity matrix top-level entry for Row 1 alongside
//! `hermes_parity_baseline.rs`.

#![cfg(feature = "integration")]

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;

use sera_e2e_harness::binaries::{gateway_bin, runtime_bin};
use sera_e2e_harness::mock_llm::{start_mock_llm_with_reply, start_mock_llm_with_reply_and_delay};
use sera_e2e_harness::{
    count_transcript_rows, multi_agent_sera_yaml, resolve_model_env, GatewayRoot,
    InProcessGateway,
};

const SKIP_TAG_LANE: &str = "[hermes-parity-api-lane-busy]";
const SKIP_TAG_ISOLATION: &str = "[hermes-parity-api-session-isolation]";

/// Bearer header that paired tests send. Auth is not enforced under the
/// autonomous-mode harness (`InProcessGateway::start_local`), so this is
/// decorative for the local CI path but forward-fit for the auth-enabled
/// harness profile tracked under `sera-baseline-auth-enforced`.
const DEFAULT_BEARER: &str = "sera_bootstrap_dev_123";

fn bearer() -> String {
    std::env::var("SERA_E2E_BEARER_TOKEN").unwrap_or_else(|_| DEFAULT_BEARER.to_owned())
}

/// Probe N+1 of Row 1: lane-busy short-circuit.
///
/// We boot the gateway against a mock LLM that sleeps for 4 seconds before
/// responding, then fire two `/api/chat` POSTs concurrently for the same
/// agent (and therefore the same gateway-owned session key). Whichever
/// request wins the lane mutex claims the slot and waits on the LLM; the
/// loser must observe `Queued` and return 429 with the documented
/// `lane_busy` envelope. The assertion is order-independent — exactly one
/// 429 and one 2xx across the pair, regardless of physical request order.
///
/// Without the LLM delay, the runtime can complete the lane-winner's turn
/// before tokio schedules the other request to the lane mutex — the race
/// is benign in production (the second request just runs serially) but
/// kills the test's lane-busy assertion, so we hold the lane open with the
/// mock delay.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hermes_parity_api_lane_busy_returns_429() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SERA_E2E_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_test_writer()
        .try_init();

    let gateway_bin_path = match gateway_bin() {
        Some(p) => p,
        None => {
            eprintln!(
                "{SKIP_TAG_LANE} SKIP: sera-gateway binary not found. \
                 Run `cargo build -p sera-gateway` first."
            );
            return Ok(());
        }
    };
    let runtime_bin_path = match runtime_bin() {
        Some(p) => p,
        None => {
            eprintln!(
                "{SKIP_TAG_LANE} SKIP: sera-runtime binary not found. \
                 Run `cargo build -p sera-runtime` first."
            );
            return Ok(());
        }
    };

    // Lane-busy probe needs the scripted mock so we can hold the lane open
    // deterministically; a real upstream LLM has no portable way to be
    // delayed. Skip cleanly when the operator points at a live LLM.
    if std::env::var("SERA_E2E_LLM_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_some()
    {
        eprintln!(
            "{SKIP_TAG_LANE} SKIP: SERA_E2E_LLM_BASE_URL is set; lane-busy \
             smoke requires the local mock so the LLM can be held open \
             deterministically."
        );
        return Ok(());
    }

    // 4 s is comfortably longer than the harness's HTTP-client timeout for
    // the second probe (the second request returns before the LLM ever
    // replies), but well inside the 15 s top-level test budget. Reducing
    // further risks the first turn completing before the second request
    // reaches the lane-queue mutex on a hot scheduler.
    let mock_reply = "OK lane-busy";
    let (llm_base_url, _mock_handle) =
        match start_mock_llm_with_reply_and_delay(mock_reply, Duration::from_secs(4)).await {
            Ok((url, server)) => (url, server),
            Err(e) => {
                eprintln!(
                    "{SKIP_TAG_LANE} SKIP: could not start local mock LLM ({e})."
                );
                return Ok(());
            }
        };

    let gateway =
        InProcessGateway::start_local(&gateway_bin_path, &runtime_bin_path, &llm_base_url)
            .await
            .context("gateway failed to boot — lane-busy parity smoke")?;

    // Single client with a generous timeout: both requests may need to wait
    // on the 4 s mock-LLM delay if they happen to be the lane winner, so the
    // timeout must cover that path. A body-read stall therefore surfaces as
    // a test assertion failure (with the body content visible) rather than
    // an opaque reqwest timeout.
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("building lane-busy HTTP client")?;

    let url = format!("{}/api/chat", gateway.base_url);
    let body = json!({
        "agent": "sera",
        "message": "Hermes parity lane-busy probe.",
        "stream": false,
    });
    let token = bearer();

    // Fire both concurrently. tokio::join! starts both futures immediately;
    // whichever reaches the lane mutex first claims it (Ready) and the
    // loser observes Queued and returns 429. The race is *not* bound to
    // request order — on a contended CI worker, scheduling and localhost
    // connect timing can flip which call wins (Codex review on PR #1276
    // r3292990638). The contract this probe enforces is therefore
    // "exactly one 429 and one success", not "the second request is 429".
    let req_a = http
        .post(&url)
        .bearer_auth(&token)
        .json(&body)
        .send();
    let req_b = http
        .post(&url)
        .bearer_auth(&token)
        .json(&body)
        .send();
    let (res_a, res_b) = tokio::join!(req_a, req_b);
    let resp_a = res_a.context("lane-busy probe A send")?;
    let resp_b = res_b.context("lane-busy probe B send")?;

    // Identify the 429 (lane loser) and the 2xx (lane winner) without
    // binding to which physical request fired first. Exactly one of each
    // is required; everything else is a regression in the lane gate.
    let status_a = resp_a.status();
    let status_b = resp_b.status();
    let (busy_resp, success_resp) = match (status_a, status_b) {
        (s_a, s_b)
            if s_a == reqwest::StatusCode::TOO_MANY_REQUESTS && s_b.is_success() =>
        {
            (resp_a, resp_b)
        }
        (s_a, s_b)
            if s_b == reqwest::StatusCode::TOO_MANY_REQUESTS && s_a.is_success() =>
        {
            (resp_b, resp_a)
        }
        _ => panic!(
            "lane-busy smoke: expected exactly one 429 and one 2xx across the \
             two concurrent /api/chat requests, got A={status_a:?} B={status_b:?}"
        ),
    };

    // The 429 must carry the documented `Retry-After` header and structured
    // body envelope. These pin the operator-visible "blocked" state.
    let retry_after = busy_resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .with_context(|| "lane-busy smoke: 429 response missing Retry-After header")?;
    assert_eq!(
        retry_after, "15",
        "lane-busy smoke: Retry-After must be 15 seconds (LANE_BUSY_RETRY_AFTER_SECS), got {retry_after:?}"
    );
    // Use `.text()` + manual JSON parse so an unexpected body shape (or a
    // body-read stall) surfaces the actual response text in the failure
    // message rather than an opaque reqwest decode error.
    let busy_body_text = busy_resp
        .text()
        .await
        .context("reading lane-busy 429 body as text")?;
    let busy_body: serde_json::Value = serde_json::from_str(&busy_body_text)
        .with_context(|| {
            format!("parsing lane-busy 429 body as JSON; raw body: {busy_body_text:?}")
        })?;
    assert_eq!(
        busy_body["error"], "rate_limited",
        "lane-busy smoke: 429 body.error must be \"rate_limited\", got {busy_body:?}"
    );
    assert_eq!(
        busy_body["reason"], "lane_busy",
        "lane-busy smoke: 429 body.reason must be \"lane_busy\", got {busy_body:?}"
    );
    assert_eq!(
        busy_body["retry_after_secs"], 15,
        "lane-busy smoke: 429 body.retry_after_secs must be 15, got {busy_body:?}"
    );

    // The lane winner must complete 2xx after the mock's delay clears. A
    // non-success here surfaces a deeper completion-path regression — for
    // example the delayed LLM call being dropped while the lane was
    // contended.
    assert!(
        success_resp.status().is_success(),
        "lane-busy smoke: lane-winning /api/chat should complete 2xx after the lane clears, got {:?}",
        success_resp.status()
    );

    // Tolerate slow shutdowns: the gateway's drain can outrun the harness's
    // 5 s shutdown deadline when the mock LLM is mid-delay or the runtime
    // child is still flushing transcript rows. The probe assertions above
    // are the contract this test enforces; a shutdown stall is a separate
    // bead (`sera-q66q` ledger drain timing). Matches the pattern used in
    // `scenarios_s2_config.rs`.
    if let Err(e) = gateway.shutdown().await {
        eprintln!("gateway shutdown returned: {e}");
    }
    Ok(())
}

/// Probe N+2 of Row 1: per-agent session isolation.
///
/// Multi-agent manifest with two agents (`sera-a`, `sera-b`). Send a unique
/// user message to each, then assert: distinct `session_id`s, distinct
/// non-empty replies, and the SQLite transcript stores each user line under
/// its owning session (and only under its owning session).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hermes_parity_api_session_isolation_per_agent() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SERA_E2E_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_test_writer()
        .try_init();

    let gateway_bin_path = match gateway_bin() {
        Some(p) => p,
        None => {
            eprintln!(
                "{SKIP_TAG_ISOLATION} SKIP: sera-gateway binary not found."
            );
            return Ok(());
        }
    };
    let runtime_bin_path = match runtime_bin() {
        Some(p) => p,
        None => {
            eprintln!(
                "{SKIP_TAG_ISOLATION} SKIP: sera-runtime binary not found."
            );
            return Ok(());
        }
    };

    let (llm_base_url, _mock_handle) = match std::env::var("SERA_E2E_LLM_BASE_URL") {
        Ok(url) if !url.trim().is_empty() => (url, None),
        _ => match start_mock_llm_with_reply("ack").await {
            Ok((url, server)) => (url, Some(server)),
            Err(e) => {
                eprintln!(
                    "{SKIP_TAG_ISOLATION} SKIP: could not start mock LLM ({e})."
                );
                return Ok(());
            }
        },
    };

    let model = resolve_model_env();
    let manifest = multi_agent_sera_yaml(&llm_base_url, &model, &["sera-a", "sera-b"]);
    let root = GatewayRoot::new_with_manifest(&manifest)
        .context("building multi-agent root for isolation smoke")?;
    let gateway = InProcessGateway::start_with_root(
        &root,
        &gateway_bin_path,
        &runtime_bin_path,
        &llm_base_url,
    )
    .await
    .context("gateway failed to boot — isolation parity smoke")?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("building isolation HTTP client")?;
    let url = format!("{}/api/chat", gateway.base_url);
    let token = bearer();

    let nonce_a = format!("ALPHA-{}", short_nonce());
    let nonce_b = format!("BETA-{}", short_nonce());
    let message_a = format!("Isolation probe for agent A nonce={nonce_a}");
    let message_b = format!("Isolation probe for agent B nonce={nonce_b}");

    // Sequential, not concurrent — the lane-busy contract is exercised
    // elsewhere. Here we want each agent's lane to settle before reading the
    // transcript so the row-count assertions are deterministic.
    let resp_a: serde_json::Value = http
        .post(&url)
        .bearer_auth(&token)
        .json(&json!({
            "agent": "sera-a",
            "message": message_a,
            "stream": false,
        }))
        .send()
        .await
        .context("agent A chat send")?
        .error_for_status()
        .context("agent A chat status")?
        .json()
        .await
        .context("agent A chat body parse")?;
    let session_a = resp_a
        .get("session_id")
        .and_then(|v| v.as_str())
        .with_context(|| format!("agent A response missing session_id: {resp_a:?}"))?
        .to_owned();
    let response_a = resp_a
        .get("response")
        .and_then(|v| v.as_str())
        .with_context(|| format!("agent A response missing response: {resp_a:?}"))?
        .to_owned();
    assert!(
        !session_a.is_empty() && !response_a.is_empty(),
        "isolation smoke: agent A must return non-empty session_id and response, got {resp_a:?}"
    );

    let resp_b: serde_json::Value = http
        .post(&url)
        .bearer_auth(&token)
        .json(&json!({
            "agent": "sera-b",
            "message": message_b,
            "stream": false,
        }))
        .send()
        .await
        .context("agent B chat send")?
        .error_for_status()
        .context("agent B chat status")?
        .json()
        .await
        .context("agent B chat body parse")?;
    let session_b = resp_b
        .get("session_id")
        .and_then(|v| v.as_str())
        .with_context(|| format!("agent B response missing session_id: {resp_b:?}"))?
        .to_owned();
    let response_b = resp_b
        .get("response")
        .and_then(|v| v.as_str())
        .with_context(|| format!("agent B response missing response: {resp_b:?}"))?
        .to_owned();
    assert!(
        !session_b.is_empty() && !response_b.is_empty(),
        "isolation smoke: agent B must return non-empty session_id and response, got {resp_b:?}"
    );

    // Core isolation invariant: the two agents get different sessions.
    assert_ne!(
        session_a, session_b,
        "isolation smoke: two agents must receive distinct session_ids, both got {session_a}"
    );

    // SQLite cross-check: each session must hold exactly one user row, and
    // the content of that row must be the message we sent to that agent —
    // never the other agent's message. The `count_transcript_rows` helper
    // does the COUNT(*); the content check uses a raw rusqlite query so the
    // assertion can pin the exact text.
    //
    // The assistant role count is checked loosely (>=1) because some
    // provider/runtime paths persist multi-segment replies (tool_call
    // followed by content) as separate rows. The user count is exact (==1)
    // because the gateway writes one user row per turn.
    let user_rows_a = count_transcript_rows(&gateway.db_path, &session_a, "user")
        .context("counting user rows for session A")?;
    let user_rows_b = count_transcript_rows(&gateway.db_path, &session_b, "user")
        .context("counting user rows for session B")?;
    assert_eq!(
        user_rows_a, 1,
        "isolation smoke: session A must have exactly one user row, got {user_rows_a}"
    );
    assert_eq!(
        user_rows_b, 1,
        "isolation smoke: session B must have exactly one user row, got {user_rows_b}"
    );

    let conn = sera_e2e_harness::rusqlite::Connection::open_with_flags(
        &gateway.db_path,
        sera_e2e_harness::rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .context("opening sera.db for isolation cross-check")?;
    let text_a: String = conn
        .query_row(
            "SELECT content FROM transcript WHERE session_id = ?1 AND role = 'user' LIMIT 1",
            sera_e2e_harness::rusqlite::params![session_a],
            |row| row.get(0),
        )
        .context("reading session A user transcript row")?;
    let text_b: String = conn
        .query_row(
            "SELECT content FROM transcript WHERE session_id = ?1 AND role = 'user' LIMIT 1",
            sera_e2e_harness::rusqlite::params![session_b],
            |row| row.get(0),
        )
        .context("reading session B user transcript row")?;
    assert!(
        text_a.contains(&nonce_a),
        "isolation smoke: session A transcript must contain nonce A ({nonce_a}); got {text_a:?}"
    );
    assert!(
        !text_a.contains(&nonce_b),
        "isolation smoke: session A transcript leaked nonce B ({nonce_b}); got {text_a:?}"
    );
    assert!(
        text_b.contains(&nonce_b),
        "isolation smoke: session B transcript must contain nonce B ({nonce_b}); got {text_b:?}"
    );
    assert!(
        !text_b.contains(&nonce_a),
        "isolation smoke: session B transcript leaked nonce A ({nonce_a}); got {text_b:?}"
    );

    // Tolerate slow shutdowns: the gateway's drain can outrun the harness's
    // 5 s shutdown deadline when the mock LLM is mid-delay or the runtime
    // child is still flushing transcript rows. The probe assertions above
    // are the contract this test enforces; a shutdown stall is a separate
    // bead (`sera-q66q` ledger drain timing). Matches the pattern used in
    // `scenarios_s2_config.rs`.
    if let Err(e) = gateway.shutdown().await {
        eprintln!("gateway shutdown returned: {e}");
    }
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
