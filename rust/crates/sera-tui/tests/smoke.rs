//! Integration smoke tests for the TUI — gated behind the `integration`
//! feature so a bare `cargo test` stays fast.
//!
//! Spins up a real `sera-gateway` binary (via `sera-e2e-harness`), points
//! a `GatewayClient` at it, and asserts the endpoints the TUI relies on
//! either return data or degrade to an empty list (never 5xx).
//!
//! Rendering assertions are covered at unit-test level inside the bin
//! crate (`src/ui.rs::tests`, `src/views/*.rs::tests`); duplicating them
//! here would require exposing the bin as a library, which we defer to
//! the planned `sera-client` extraction.
//!
//! The binary locator is identical in spirit to the one
//! `sera-e2e-harness`'s own integration test uses — Cargo only sets
//! `CARGO_BIN_EXE_*` for bins in the *same* crate, so both tests walk
//! out from their own binary path and probe for siblings.

#![cfg(feature = "integration")]
// The repathed `client` module re-imports the full TUI client surface
// but this single test only exercises a subset.  Silence dead_code +
// unused_imports for the module so the integration build stays clean.
#![allow(dead_code, unused_imports)]

#[path = "../src/client.rs"]
mod client;

use std::path::PathBuf;
use std::time::Duration;

use sera_e2e_harness::InProcessGateway;

/// Walk up from the running test binary and look for a workspace bin
/// named `name`.  Returns `None` when the bin hasn't been built yet so
/// tests can skip cleanly on cold cargo invocations.
fn locate_workspace_bin(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut cur = exe.as_path();
    for _ in 0..4 {
        cur = cur.parent()?;
        let candidate = {
            #[cfg(windows)]
            {
                cur.join(format!("{name}.exe"))
            }
            #[cfg(not(windows))]
            {
                cur.join(name)
            }
        };
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tui_client_round_trips_against_local_profile() {
    let Some(gateway_bin) = locate_workspace_bin("sera-gateway") else {
        eprintln!(
            "SKIP: sera-gateway binary not found — run `cargo build -p sera-gateway -p sera-runtime` first"
        );
        return;
    };
    let Some(runtime_bin) = locate_workspace_bin("sera-runtime") else {
        eprintln!(
            "SKIP: sera-runtime binary not found — run `cargo build -p sera-runtime` first"
        );
        return;
    };

    // Wiremock satisfies the gateway's startup LLM-URL requirement even
    // though we never issue a turn in this smoke test.
    let mock = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::any())
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&mock)
        .await;

    let gw = InProcessGateway::start_local(&gateway_bin, &runtime_bin, &mock.uri())
        .await
        .expect("gateway should boot within BOOT_DEADLINE");

    let gateway_client = client::GatewayClient::new(
        &gw.base_url,
        "sera_bootstrap_dev_123",
        Duration::from_secs(5),
    )
    .unwrap();

    // 1. /api/health must succeed.
    let _ = gateway_client
        .health()
        .await
        .expect("/api/health should 200");

    // 2. /api/agents returns the single manifest agent.
    let agents = gateway_client
        .list_agents()
        .await
        .expect("/api/agents should respond");
    assert!(!agents.is_empty(), "expected at least one agent");
    assert!(
        agents.iter().any(|a| a.name == "sera"),
        "expected manifest agent 'sera', got {:?}",
        agents.iter().map(|a| &a.name).collect::<Vec<_>>()
    );

    // 3. /api/permission-requests — autonomous gateway doesn't mount
    // this route; the client must degrade to an empty list, not error.
    let hitl = gateway_client
        .list_hitl()
        .await
        .expect("HITL client must degrade cleanly on 404");
    assert!(hitl.is_empty(), "expected no HITL requests, got {hitl:?}");

    // 4. Evolve — same degrade contract.
    let proposals = gateway_client
        .list_evolve_proposals()
        .await
        .expect("evolve client must degrade cleanly on 404");
    assert!(
        proposals.is_empty(),
        "expected no evolve proposals, got {proposals:?}"
    );

    // 5. /api/sessions — autonomous gateway supports this; it may be
    // empty until a turn runs, but the call itself must not error.
    let _sessions = gateway_client
        .list_sessions(None)
        .await
        .expect("/api/sessions should respond");

    let _ = gw.shutdown().await;
}
