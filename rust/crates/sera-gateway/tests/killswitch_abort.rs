//! Integration test for the KillSwitch → in-flight turn abort wiring
//! (sera-bsem).
//!
//! The gateway binary keeps a `CancellationToken` per in-flight turn/steer in
//! `AppState.active_cancellation_tokens` and drives it from the admin socket's
//! `on_rollback` closure. The `AppState` type itself lives in the binary and
//! is not reachable from an integration test, so this test exercises the same
//! pattern against the public `KillSwitch` + `spawn_admin_socket` surface:
//! register a `CancellationToken`, issue `ROLLBACK` over the admin socket,
//! assert the token is cancelled within <1 s.

#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use sera_gateway::kill_switch::{KillSwitch, spawn_admin_socket};
use tokio_util::sync::CancellationToken;

/// A ROLLBACK command arriving on the admin socket must cancel every
/// registered token (simulating an in-flight `execute_turn`) within a short
/// bound. Regression guard for the "armed KillSwitch does not abort wedged
/// turns" bug: pre-fix the kill switch only flipped a flag, so turns stayed
/// pinned to their lane slots even while the gateway was supposedly halted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rollback_cancels_registered_tokens_within_one_second() {
    let ks = Arc::new(KillSwitch::new());
    let path = format!("/tmp/sera-test-bsem-abort-{}.sock", std::process::id());

    // Simulate AppState.active_cancellation_tokens: a shared map of tokens
    // keyed by session_key. The rollback callback drains and cancels all of
    // them exactly like `AppState::cancel_all_in_flight` does in the
    // production boot path.
    let registry: Arc<Mutex<std::collections::HashMap<String, CancellationToken>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));

    // Register three in-flight tokens before the rollback arrives.
    let token_a = CancellationToken::new();
    let token_b = CancellationToken::new();
    let token_c = CancellationToken::new();
    {
        let mut map = registry.lock().unwrap();
        map.insert("session-a".to_string(), token_a.clone());
        map.insert("session-b".to_string(), token_b.clone());
        map.insert("session-c".to_string(), token_c.clone());
    }

    // Mirror the production on_rollback: drain and cancel every token.
    let on_rollback_registry = Arc::clone(&registry);
    spawn_admin_socket(Arc::clone(&ks), path.clone(), move || {
        let mut map = on_rollback_registry.lock().unwrap();
        for (_key, token) in map.drain() {
            token.cancel();
        }
    });

    // Give the listener a moment to bind.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Spawn a task per token that waits on `cancelled()` and records the
    // elapsed time — this is what `execute_turn`'s cancellation arm does.
    let start = std::time::Instant::now();
    let wait_a = tokio::spawn({
        let t = token_a.clone();
        async move { t.cancelled().await }
    });
    let wait_b = tokio::spawn({
        let t = token_b.clone();
        async move { t.cancelled().await }
    });
    let wait_c = tokio::spawn({
        let t = token_c.clone();
        async move { t.cancelled().await }
    });

    // Fire ROLLBACK.
    let mut stream = UnixStream::connect(&path).expect("connect to admin socket");
    stream.write_all(b"ROLLBACK\n").expect("write ROLLBACK");
    let mut resp = String::new();
    stream.read_to_string(&mut resp).expect("read response");
    assert_eq!(resp, "OK\n");

    // All three tokens must complete `cancelled()` within one second of the
    // admin socket command — the bug the fix addresses is turns that stay
    // running for minutes after ROLLBACK.
    let bound = Duration::from_secs(1);
    tokio::time::timeout(bound, async {
        wait_a.await.unwrap();
        wait_b.await.unwrap();
        wait_c.await.unwrap();
    })
    .await
    .expect("every token must be cancelled within 1s of ROLLBACK");

    assert!(ks.is_armed());
    assert!(
        start.elapsed() < bound,
        "cancellation must fire within the advertised bound"
    );
    assert!(token_a.is_cancelled());
    assert!(token_b.is_cancelled());
    assert!(token_c.is_cancelled());

    // The registry must be empty afterwards — mirrors the clear semantics of
    // `AppState::cancel_all_in_flight`, which drain()s the map so stale
    // entries cannot accumulate across rollback cycles.
    assert!(
        registry.lock().unwrap().is_empty(),
        "rollback callback must drain the registry"
    );

    let _ = std::fs::remove_file(&path);
}

/// Tokens registered AFTER a ROLLBACK are not retroactively cancelled (the
/// drain happens once; later entries live until their execute_turn completes
/// or the next ROLLBACK). This documents the semantics explicitly so a future
/// refactor doesn't accidentally couple the armed flag to the map.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rollback_does_not_retroactively_cancel_late_registrations() {
    let ks = Arc::new(KillSwitch::new());
    let path = format!("/tmp/sera-test-bsem-late-{}.sock", std::process::id());
    let registry: Arc<Mutex<std::collections::HashMap<String, CancellationToken>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));

    let on_rollback_registry = Arc::clone(&registry);
    spawn_admin_socket(Arc::clone(&ks), path.clone(), move || {
        let mut map = on_rollback_registry.lock().unwrap();
        for (_key, token) in map.drain() {
            token.cancel();
        }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Arm the kill switch with an empty registry.
    {
        let mut stream = UnixStream::connect(&path).unwrap();
        stream.write_all(b"ROLLBACK\n").unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        assert_eq!(resp, "OK\n");
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(ks.is_armed());

    // Register a token after the rollback. With the armed flag guarding the
    // HTTP submission path in production, a new turn should never be admitted
    // here — this test just documents the token lifecycle: the token is live
    // and not cancelled because the drain ran before it was inserted.
    let late_token = CancellationToken::new();
    registry
        .lock()
        .unwrap()
        .insert("late".to_string(), late_token.clone());

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !late_token.is_cancelled(),
        "tokens registered after ROLLBACK are not retroactively cancelled"
    );

    let _ = std::fs::remove_file(&path);
}
