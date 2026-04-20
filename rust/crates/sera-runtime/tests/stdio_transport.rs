//! P0-6 acceptance test #15 — spawn the `sera-runtime` binary, send an NDJSON
//! `Shutdown` submission, and assert the subprocess emits a canonical
//! `HandshakeFrame` on its first line and exits cleanly.
//!
//! Covers the `AppServerTransport::Stdio` contract from the runtime side
//! without requiring a live LLM: the submission is a `System::Shutdown` which
//! short-circuits the turn loop, so no LLM call happens.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;

use sera_types::envelope::HandshakeFrame;

/// Locate the just-built `sera-runtime` binary. Cargo sets `CARGO_BIN_EXE_sera-runtime`
/// for integration tests in this crate.
fn binary_path() -> String {
    env!("CARGO_BIN_EXE_sera-runtime").to_string()
}

#[tokio::test]
async fn main_binary_boots_under_stdio_transport() {
    let bin = binary_path();
    let mut child = Command::new(&bin)
        .arg("--ndjson")
        .arg("--no-health")
        // Minimal env so the runtime can build its config without touching network.
        .env("AGENT_ID", "stdio-acceptance-agent")
        .env("LLM_BASE_URL", "http://127.0.0.1:1")
        .env("LLM_MODEL", "mock-model")
        .env("LLM_API_KEY", "mock-key")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sera-runtime");

    let mut stdin = child.stdin.take().expect("stdin piped");
    let stdout = child.stdout.take().expect("stdout piped");
    let mut reader = BufReader::new(stdout);

    // First line MUST be the canonical HandshakeFrame per P0-6 §main.rs rewrite.
    let mut line = String::new();
    let read = timeout(Duration::from_secs(10), reader.read_line(&mut line))
        .await
        .expect("handshake read timed out")
        .expect("read handshake line");
    assert!(read > 0, "runtime produced no handshake line");

    let frame: HandshakeFrame =
        serde_json::from_str(line.trim()).expect("first line must be a HandshakeFrame");
    assert_eq!(frame.protocol_version, "2.0", "handshake must advertise v2 protocol");
    assert_eq!(frame.frame_type, "handshake");
    assert_eq!(
        frame.agent_id.as_deref(),
        Some("stdio-acceptance-agent"),
        "handshake must echo the agent id"
    );
    assert!(frame.capabilities.supports("steer"));
    assert!(frame.capabilities.supports("hitl"));
    assert!(frame.capabilities.supports("hooks@v2"));

    // Send a `System::Shutdown` submission and assert the subprocess exits.
    let shutdown = serde_json::json!({
        "id": uuid::Uuid::new_v4(),
        "op": { "type": "system", "system_op": "shutdown" }
    });
    let mut json = serde_json::to_string(&shutdown).unwrap();
    json.push('\n');
    stdin.write_all(json.as_bytes()).await.unwrap();
    stdin.flush().await.unwrap();
    drop(stdin); // close stdin so the runtime can see EOF too

    let status = timeout(Duration::from_secs(10), child.wait())
        .await
        .expect("runtime did not exit within timeout")
        .expect("wait for runtime exit");
    assert!(status.success(), "runtime exited non-zero: {status:?}");
}
