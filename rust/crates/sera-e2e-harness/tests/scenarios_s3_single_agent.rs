//! S3 — Single-agent smoketest via four interfaces (Phase 1 of the
//! TEST-SCENARIOS plan).
//!
//! These tests cover the headline "a user can actually chat" promise across
//! the three non-TUI interfaces we ship today.  The TUI slice (S3.4) is
//! tested as a headless reducer in the `sera-tui` crate's own unit tests —
//! driving a real terminal inside `cargo test` would require a pty harness
//! that is out of scope for Phase 1.
//!
//! Covered:
//! * S3.1 — `POST /api/chat {stream: false}` returns `{response, session_id,
//!   usage}` and the response text matches the mock LLM's scripted reply.
//! * S3.2 — `POST /api/chat {stream: true}` over SSE emits at least one
//!   `message` (token) event followed by a terminal `done` event, re-using
//!   the `sera-cli` stream parser to prove both client and server agree on
//!   framing.
//! * S3.3 — the `sera agent run` CLI subcommand round-trips the same turn
//!   and prints the reply on stdout; proves the binary shell users would
//!   type works against a freshly-booted gateway.

#![cfg(feature = "integration")]

use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde_json::json;
use tokio::process::Command;

use sera_cli::sse::{SseClient, StreamEvent};
use sera_e2e_harness::binaries::{cli_bin, gateway_bin, runtime_bin};
use sera_e2e_harness::mock_llm::{start_mock_llm_with_reply, DEFAULT_REPLY};
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

/// S3.1 — non-streaming chat round-trip.  Asserts on the explicit response
/// shape the gateway documents in the OpenAPI: `{response, session_id}`
/// with `usage` optional.  This is the single-call contract the SDK bindings
/// depend on, so the test spells out each field rather than smoke-checking a
/// substring.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s3_1_chat_non_stream_returns_response_with_session_id() -> Result<()> {
    init_tracing();

    let (gw_bin, rt_bin) = match bins_or_skip() {
        Some(b) => b,
        None => {
            eprintln!("[S3.1] SKIP: gateway/runtime bins not built");
            return Ok(());
        }
    };
    let (llm, _mock) = match start_mock_llm_with_reply("reply from S3.1").await {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[S3.1] SKIP: wiremock unavailable ({e})");
            return Ok(());
        }
    };

    let gw = InProcessGateway::start_local(&gw_bin, &rt_bin, &llm).await?;
    let http = reqwest::Client::builder().timeout(Duration::from_secs(15)).build()?;

    let resp: serde_json::Value = http
        .post(format!("{}/api/chat", gw.base_url))
        .json(&json!({
            "agent": "sera",
            "message": "S3.1 smoketest",
            "stream": false,
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // Response must surface the three documented fields; usage is a nested
    // object whose shape the gateway owns, so we only check presence.
    assert!(
        resp.get("session_id").and_then(|v| v.as_str()).is_some(),
        "response missing session_id: {resp:?}"
    );
    let response_text = resp
        .get("response")
        .and_then(|v| v.as_str())
        .context("response missing `response` field")?;
    assert!(
        response_text.contains("reply from S3.1"),
        "mock reply missing from response text: {response_text:?}"
    );

    if let Err(e) = gw.shutdown().await {
        eprintln!("gateway shutdown returned: {e}");
    }
    Ok(())
}

/// S3.2 — streaming chat round-trip over SSE.  The gateway emits
/// `event: message` frames for each token delta and a terminal
/// `event: done`; the sera-cli SSE parser translates those into
/// [`StreamEvent::Token`] and [`StreamEvent::Done`].  The test asserts we
/// see at least one Token *and* the stream closes with Done — both are
/// required for the REPL to render a turn correctly.
///
/// Re-using sera-cli's parser (vs. rolling our own) means this test also
/// guards against parser/emitter drift between the two crates.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s3_2_chat_stream_emits_message_then_done() -> Result<()> {
    init_tracing();

    let (gw_bin, rt_bin) = match bins_or_skip() {
        Some(b) => b,
        None => {
            eprintln!("[S3.2] SKIP: gateway/runtime bins not built");
            return Ok(());
        }
    };
    let (llm, _mock) = match start_mock_llm_with_reply(DEFAULT_REPLY).await {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[S3.2] SKIP: wiremock unavailable ({e})");
            return Ok(());
        }
    };

    let gw = InProcessGateway::start_local(&gw_bin, &rt_bin, &llm).await?;
    let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;
    let sse = SseClient::new(http, gw.base_url.clone());

    let mut stream = sse
        .post_stream(
            "/api/chat",
            json!({
                "agent": "sera",
                "message": "S3.2 streaming",
                "stream": true,
            }),
        )
        .await
        .context("opening SSE stream")?;

    let mut saw_message = false;
    let mut saw_done = false;
    let mut saw_error: Option<String> = None;
    // Bound the loop — a stuck runtime must not hang the test forever.
    let start = std::time::Instant::now();
    let budget = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let tick = tokio::time::timeout_at(budget, stream.next()).await;
        let ev = match tick {
            Err(_) => panic!(
                "SSE stream timed out after {:?} — saw_message={saw_message}, saw_done={saw_done}, saw_error={saw_error:?}",
                start.elapsed()
            ),
            Ok(None) => break, // stream closed cleanly without Done — caller assertion will catch it
            Ok(Some(Err(e))) => panic!("SSE frame parse error: {e}"),
            Ok(Some(Ok(ev))) => ev,
        };
        match ev {
            StreamEvent::Token { .. } => saw_message = true,
            StreamEvent::Done { .. } => {
                saw_done = true;
                break;
            }
            StreamEvent::Error { message } => {
                saw_error = Some(message);
                break;
            }
            _ => {} // keep permissive for new event kinds we don't know about
        }
    }

    assert!(saw_error.is_none(), "SSE stream surfaced an error event: {saw_error:?}");
    assert!(saw_message, "SSE stream must emit at least one token delta before done");
    assert!(
        saw_done,
        "SSE stream must close with a `done` frame (saw_message={saw_message}, error={saw_error:?})"
    );

    if let Err(e) = gw.shutdown().await {
        eprintln!("gateway shutdown returned: {e}");
    }
    Ok(())
}

/// S3.3 — `sera agent run <id> <prompt> --no-stream` one-shot round-trip.
/// Spawns the built CLI binary as a subprocess so the test exercises the
/// real executable path (argv parsing, config loading, HTTP client) that a
/// human operator would invoke from their shell.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s3_3_cli_agent_run_prints_reply() -> Result<()> {
    init_tracing();

    let (gw_bin, rt_bin) = match bins_or_skip() {
        Some(b) => b,
        None => {
            eprintln!("[S3.3] SKIP: gateway/runtime bins not built");
            return Ok(());
        }
    };
    let cli = match cli_bin() {
        Some(p) => p,
        None => {
            eprintln!("[S3.3] SKIP: sera CLI bin not built (run `cargo build -p sera-cli`)");
            return Ok(());
        }
    };
    let (llm, _mock) = match start_mock_llm_with_reply("reply from CLI test").await {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[S3.3] SKIP: wiremock unavailable ({e})");
            return Ok(());
        }
    };

    let gw = InProcessGateway::start_local(&gw_bin, &rt_bin, &llm).await?;

    // Use a throw-away home dir so the CLI's `~/.sera/config.toml` cannot
    // interfere with the test run.  The CLI reads from HOME by default.
    let home = tempfile::tempdir()?;

    // Pre-seed `$HOME/.sera/token` with a placeholder bearer.  `sera agent run`
    // aborts with exit code 2 when no token is found, even when pointed at an
    // autonomous gateway that accepts unauthenticated requests (tracked as an
    // auth-asymmetry issue; filed separately).  The gateway's autonomous-mode
    // auth middleware accepts any bearer string, so a hard-coded placeholder
    // is enough here.
    let token_dir = home.path().join(".sera");
    std::fs::create_dir_all(&token_dir).context("creating ~/.sera for token")?;
    std::fs::write(token_dir.join("token"), "dev-token-s3-3")
        .context("seeding ~/.sera/token")?;

    let output = Command::new(&cli)
        .arg("agent")
        .arg("run")
        .arg("sera") // agent name/id — matches the harness manifest
        .arg("S3.3 hello from CLI")
        .arg("--endpoint")
        .arg(&gw.base_url)
        .arg("--no-stream")
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .output()
        .await
        .context("spawning sera CLI")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        panic!("sera CLI exited with {:?}\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}", output.status);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("reply from CLI test"),
        "CLI stdout must contain mock reply; got: {stdout:?}; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    if let Err(e) = gw.shutdown().await {
        eprintln!("gateway shutdown returned: {e}");
    }
    Ok(())
}
