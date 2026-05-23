//! Phase 0 Hermes parity baseline gate.
//!
//! Companion test to `docs/internal/plans/hermes-parity-matrix.md`. This is
//! the smallest live-baseline bundle that proves SERA can host a turn at
//! all, with three probes:
//!
//! 1. `GET /api/health/ready` returns 200 with `runtime_connected=true`.
//! 2. Authenticated `POST /api/chat` returns 200 with a `session_id` and a
//!    non-empty `response` for a nonce prompt.
//! 3. The operator-visible `response` body contains no `<think>` /
//!    `</think>` markers — the response sanitizer must not leak raw
//!    chain-of-thought to the operator.
//!
//! Skip contract mirrors `local_profile_turn.rs`: if the gateway or
//! runtime binaries cannot be located, or if a mock LLM cannot be
//! started and `SERA_E2E_LLM_BASE_URL` is unset, the test prints an
//! explicit skip line to stderr and returns `Ok(())` rather than
//! failing.

#![cfg(feature = "integration")]

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;

use sera_e2e_harness::binaries::{gateway_bin, runtime_bin};
use sera_e2e_harness::mock_llm::start_mock_llm;
use sera_e2e_harness::InProcessGateway;

const SKIP_TAG: &str = "[hermes-parity-baseline]";

/// The Hermes parity baseline gate.
///
/// One test = three probes against a single freshly booted gateway.
/// Each probe asserts a single contract and produces a clear failure
/// message; the test does *not* gate parity for any deeper Hermes
/// capability (tools, memory, delegation, …) — those rows in the
/// parity matrix get their own gates as they land.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hermes_parity_baseline_gate() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SERA_E2E_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_test_writer()
        .try_init();

    // ── 0. Locate the gateway + runtime binaries ──
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

    // ── 1. Resolve an LLM endpoint (operator-supplied or local mock) ──
    //
    // `start_mock_llm` returns `(url, server)` and the caller must keep
    // `server` alive for the duration of the test — dropping it tears
    // down the wiremock listener. The `_mock_handle` binding here is
    // exactly that lifetime anchor; it is intentionally unused.
    let (llm_base_url, _mock_handle) = match std::env::var("SERA_E2E_LLM_BASE_URL") {
        Ok(url) if !url.trim().is_empty() => (url, None),
        _ => match start_mock_llm().await {
            Ok((url, server)) => (url, Some(server)),
            Err(e) => {
                eprintln!(
                    "{SKIP_TAG} SKIP: could not start local mock LLM and \
                     SERA_E2E_LLM_BASE_URL is unset ({e}). Baseline gate \
                     needs either a real LLM or a wiremock bind."
                );
                return Ok(());
            }
        },
    };

    // ── 2. Boot the gateway child process ──
    //
    // Once we are past the binary-located + LLM-resolved gates above,
    // the environment is capable of running this test, and a boot
    // failure here is a real parity regression (panic, config drift,
    // runtime wiring break, port collision). Propagate the error so
    // CI fails loudly instead of skipping. The "expected on stripped
    // CI environments" path is handled by the two skips above.
    let gateway = InProcessGateway::start_local(
        &gateway_bin_path,
        &runtime_bin_path,
        &llm_base_url,
    )
    .await
    .context("gateway failed to boot — baseline parity regression")?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("building baseline HTTP client")?;

    // ── Probe 1: /api/health/ready ──
    //
    // The gateway distinguishes "process up" (`/api/health`) from
    // "runtime usable" (`/api/health/ready`), and the parity baseline
    // requires the runtime probe to be green — `runtime_connected=true`.
    let ready: serde_json::Value = http
        .get(format!("{}/api/health/ready", gateway.base_url))
        .send()
        .await
        .context("readiness probe send")?
        .error_for_status()
        .context("readiness probe HTTP status")?
        .json()
        .await
        .context("readiness probe JSON parse")?;
    let runtime_connected = ready
        .get("runtime_connected")
        .and_then(|v| v.as_bool())
        .with_context(|| {
            format!("readiness response missing runtime_connected bool: {ready:?}")
        })?;
    assert!(
        runtime_connected,
        "baseline gate requires runtime_connected=true on /api/health/ready, got {ready:?}"
    );

    // ── Probe 2: authenticated /api/chat nonce ──
    //
    // We embed a short non-cryptographic nonce in the prompt so a fresh
    // session cannot be satisfied by a stale cached reply. Content
    // equality is intentionally not asserted — provider/runtime wiring
    // may legitimately rephrase — but the response shape (session_id
    // present, response field present) must hold.
    //
    // The request carries an `Authorization: Bearer …` header matching
    // the development bootstrap key documented in the top-level
    // CLAUDE.md. The harness boots in autonomous mode where auth is
    // not enforced, so this header is decorative for the local skip
    // path; once auth-enabled harness profiles land, the same call
    // gates correctly against the principal middleware without test
    // edits. `SERA_E2E_BEARER_TOKEN` can override the bootstrap value
    // for operator-run captures against a non-autonomous gateway.
    let bearer_token = std::env::var("SERA_E2E_BEARER_TOKEN")
        .unwrap_or_else(|_| "sera_bootstrap_dev_123".to_string());
    let nonce = short_nonce();
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
        .context("chat nonce send")?
        .error_for_status()
        .context("chat nonce HTTP status")?
        .json()
        .await
        .context("chat nonce JSON parse")?;

    let session_id = chat
        .get("session_id")
        .and_then(|v| v.as_str())
        .with_context(|| format!("chat response missing session_id: {chat:?}"))?
        .to_owned();
    assert!(
        !session_id.is_empty(),
        "baseline gate requires a non-empty session_id, got {chat:?}"
    );

    let response_text = chat
        .get("response")
        .and_then(|v| v.as_str())
        .with_context(|| format!("chat response missing response: {chat:?}"))?
        .to_owned();

    // The parity baseline requires a non-empty operator-visible reply.
    // An empty `response` makes Probe 3 (sanitizer contract) vacuous —
    // a leak would be hidden because there is nothing to scan — and the
    // matrix explicitly defines this row as "session_id + non-empty
    // response". Hard-fail here so a provider/runtime regression that
    // swallows the reply cannot ship as a green baseline. The session
    // hint in the message points at `/api/sessions/<id>/transcript` so
    // an operator can pull runtime evidence without re-running.
    assert!(
        !response_text.is_empty(),
        "baseline gate: /api/chat returned empty response — Probe 2 \
         requires non-empty operator-visible text. Inspect \
         /api/sessions/{session_id}/transcript for runtime evidence."
    );

    // ── Probe 3: response sanitizer contract ──
    //
    // Operator-visible `response` must never contain raw chain-of-thought
    // tags. Reasoning models (Qwen, DeepSeek-R1, GLM-4.6, …) emit
    // `<think>` / `</think>` blocks; SERA's runtime sanitizer is
    // responsible for stripping them before the gateway returns the
    // reply. We check both opening and closing markers — a truncated
    // stream could leak only one.
    let lc = response_text.to_ascii_lowercase();
    assert!(
        !lc.contains("<think>"),
        "baseline gate: operator-visible response leaked `<think>` marker: \
         {response_text:?}"
    );
    assert!(
        !lc.contains("</think>"),
        "baseline gate: operator-visible response leaked `</think>` marker: \
         {response_text:?}"
    );

    gateway.shutdown().await.context("graceful gateway shutdown")?;
    Ok(())
}

/// Tiny non-cryptographic nonce — process pid + nanoseconds is enough to
/// keep prompts unique across reruns of this test in the same CI job.
/// We deliberately avoid pulling in `rand` for one number.
fn short_nonce() -> String {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{pid:x}{nanos:x}")
}
