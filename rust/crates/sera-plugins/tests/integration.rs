//! End-to-end integration test: manifest → registry → circuit breaker.
//!
//! Exercises the full plugin lifecycle without any real gRPC transport:
//! 1. Parse a manifest YAML
//! 2. Register the resulting descriptor in `InMemoryPluginRegistry`
//! 3. Wrap calls with `CircuitBreaker` and verify state transitions
//! 4. Update health and confirm the registry reflects the change

use sera_plugins::{
    CircuitBreaker, CircuitState, InMemoryPluginRegistry, PluginCapability, PluginHealth,
    PluginRegistry, PluginTransport, manifest::PluginManifest,
};
use std::time::Duration;

const YAML: &str = r#"
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: integ-plugin
spec:
  capabilities:
    - ToolExecutor
  transport: grpc
  grpc:
    endpoint: "localhost:19090"
  health_check_interval: 30s
  version: "2.0.1"
"#;

/// Parse manifest, register plugin, update health, query by capability.
#[tokio::test]
async fn full_lifecycle_manifest_to_registry() {
    // (a) Parse the manifest
    let manifest = PluginManifest::from_yaml(YAML).expect("manifest must parse");
    let registration = manifest
        .into_registration()
        .expect("registration must succeed");

    assert_eq!(registration.name, "integ-plugin");
    assert_eq!(
        registration.capabilities,
        vec![PluginCapability::ToolExecutor]
    );
    assert_eq!(registration.health_check_interval, Duration::from_secs(30));
    // Verify transport shape
    match &registration.transport {
        PluginTransport::Grpc { grpc } => assert_eq!(grpc.endpoint, "localhost:19090"),
        other => panic!("expected Grpc transport, got {other:?}"),
    }

    // (b) Register in the in-memory registry
    let registry = InMemoryPluginRegistry::new();
    registry
        .register(registration)
        .await
        .expect("register must succeed");

    // Confirm it is retrievable
    let info = registry
        .get("integ-plugin")
        .await
        .expect("plugin must be found");
    assert_eq!(info.registration.name, "integ-plugin");
    assert!(!info.health.healthy, "initial health must be false");

    // (c) Update health to healthy
    registry
        .update_health("integ-plugin", PluginHealth::ok(5))
        .await
        .expect("health update must succeed");

    let info = registry.get("integ-plugin").await.unwrap();
    assert!(info.health.healthy);
    assert_eq!(info.health.latency_ms, Some(5));

    // Capability lookup must return the plugin
    let by_cap = registry
        .find_by_capability(&PluginCapability::ToolExecutor)
        .await;
    assert_eq!(by_cap.len(), 1);
    assert_eq!(by_cap[0].registration.name, "integ-plugin");
}

/// Circuit breaker: Closed → Open after threshold, then HalfOpen after timeout,
/// then back to Closed on success.
#[test]
fn circuit_breaker_state_machine() {
    let plugin_name = "integ-plugin";
    let failure_threshold = 2;
    let reset_timeout = Duration::from_millis(20);

    let cb = CircuitBreaker::new(plugin_name, failure_threshold, reset_timeout);

    // (c-i) Starts closed — calls allowed
    assert_eq!(cb.state(), CircuitState::Closed);
    assert!(cb.allow().is_ok());

    // Simulate failures up to the threshold
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Closed); // not yet open
    cb.record_failure(); // threshold reached

    // (c-ii) Now open — calls rejected
    assert_eq!(cb.state(), CircuitState::Open);
    let err = cb.allow().unwrap_err();
    assert!(
        err.to_string().contains(plugin_name),
        "error must mention plugin name"
    );

    // (c-iii) After reset_timeout the breaker moves to HalfOpen
    std::thread::sleep(reset_timeout + Duration::from_millis(10));
    assert_eq!(cb.state(), CircuitState::HalfOpen);
    assert!(cb.allow().is_ok(), "HalfOpen must allow a probe call");

    // (d) A successful probe closes the circuit
    cb.record_success();
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 0);
}

/// Deregister removes the plugin; subsequent get returns NotFound.
#[tokio::test]
async fn deregister_removes_plugin() {
    let registry = InMemoryPluginRegistry::new();
    let manifest = PluginManifest::from_yaml(YAML).unwrap();
    registry
        .register(manifest.into_registration().unwrap())
        .await
        .unwrap();

    registry
        .deregister("integ-plugin")
        .await
        .expect("deregister must succeed");

    let err = registry.get("integ-plugin").await.unwrap_err();
    assert!(
        matches!(err, sera_plugins::PluginError::PluginNotFound { .. }),
        "expected PluginNotFound, got {err:?}"
    );
}
