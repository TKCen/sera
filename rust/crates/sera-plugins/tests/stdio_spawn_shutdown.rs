//! Smoke test: spawn a real stdio plugin subprocess (using `/bin/cat` as a
//! stand-in), register it, send a heartbeat, then gracefully shut it down.
//!
//! `/bin/cat` echoes stdin to stdout, so any JSON-RPC line we write comes
//! back immediately — sufficient to exercise the framed-JSON-RPC handshake
//! path in the registry without a real plugin binary.
//!
//! MUST use `flavor = "multi_thread"` — the heartbeat path does blocking I/O
//! inside an async body; a single-threaded runtime deadlocks.

use sera_plugins::{
    InMemoryPluginRegistry, PluginCapability, PluginRegistration, PluginRegistry, PluginTransport,
    PluginVersion, StdioTransportConfig,
};
use std::collections::HashMap;
use std::time::Duration;

fn cat_registration() -> PluginRegistration {
    PluginRegistration {
        name: "cat-plugin".into(),
        version: PluginVersion::new(1, 0, 0),
        capabilities: vec![PluginCapability::ToolExecutor],
        transport: PluginTransport::Stdio {
            stdio: StdioTransportConfig {
                command: vec!["/bin/cat".into()],
                env: HashMap::new(),
            },
        },
        health_check_interval: Duration::from_secs(30),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_plugin_registers_and_deregisters() {
    let registry = InMemoryPluginRegistry::new();

    // Register spawns the subprocess
    registry
        .register(cat_registration())
        .await
        .expect("register must succeed");

    // The plugin is in the registry
    let info = registry
        .get("cat-plugin")
        .await
        .expect("plugin must be retrievable after register");
    assert_eq!(info.registration.name, "cat-plugin");

    // Deregister shuts down the subprocess (SIGTERM → wait → SIGKILL)
    registry
        .deregister("cat-plugin")
        .await
        .expect("deregister must succeed");

    // Plugin is gone from the registry
    let err = registry.get("cat-plugin").await.unwrap_err();
    assert!(matches!(
        err,
        sera_plugins::PluginError::PluginNotFound { .. }
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_heartbeat_roundtrip() {
    let registry = InMemoryPluginRegistry::new();
    registry
        .register(cat_registration())
        .await
        .expect("register must succeed");

    // `/bin/cat` echoes stdin → the heartbeat JSON-RPC line comes back as the
    // "response", which is enough to exercise the read path without error.
    let result = registry.stdio_heartbeat("cat-plugin").await;
    assert!(
        result.is_ok(),
        "heartbeat must succeed with /bin/cat: {result:?}"
    );

    // Clean up
    registry.deregister("cat-plugin").await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_duplicate_registration_rejected() {
    let registry = InMemoryPluginRegistry::new();
    registry.register(cat_registration()).await.unwrap();

    let err = registry
        .register(cat_registration())
        .await
        .expect_err("duplicate registration must be rejected");
    assert!(matches!(
        err,
        sera_plugins::PluginError::RegistrationFailed { .. }
    ));

    // Clean up the first one
    registry.deregister("cat-plugin").await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_relative_command_registration_fails() {
    let registry = InMemoryPluginRegistry::new();
    let reg = PluginRegistration {
        name: "bad-plugin".into(),
        version: PluginVersion::new(1, 0, 0),
        capabilities: vec![PluginCapability::ToolExecutor],
        transport: PluginTransport::Stdio {
            stdio: StdioTransportConfig {
                // Relative path — must be rejected per §6.2
                command: vec!["cat".into()],
                env: HashMap::new(),
            },
        },
        health_check_interval: Duration::from_secs(30),
    };

    let err = registry
        .register(reg)
        .await
        .expect_err("relative command must be rejected");
    assert!(matches!(
        err,
        sera_plugins::PluginError::RegistrationFailed { .. }
    ));
}
