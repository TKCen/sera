//! Tests for dual-transport manifest parsing (SPEC-plugins §8).
//!
//! Uses the YAML examples verbatim from §8.  Also tests the negative case:
//! a stdio manifest with a relative `command[0]` must be rejected with a
//! clear error message (§6.2 binary pinning).

use sera_plugins::{PluginCapability, PluginError, PluginTransport, manifest::PluginManifest};
use std::time::Duration;

// ── gRPC manifest (from SPEC-plugins §8) ──────────────────────────────────

const GRPC_YAML: &str = r#"
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: my-grpc-plugin
spec:
  capabilities: [ToolExecutor]
  transport: grpc
  grpc:
    endpoint: "localhost:9090"
    tls:
      ca_cert: /etc/sera/plugins/ca.crt
      client_cert: /etc/sera/plugins/client.crt
      client_key: /etc/sera/plugins/client.key
  health_check_interval: 30s
"#;

// ── stdio manifest (from SPEC-plugins §8) ────────────────────────────────

const STDIO_YAML: &str = r#"
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: my-stdio-plugin
spec:
  capabilities: [ContextEngine]
  transport: stdio
  stdio:
    command: ["/usr/bin/python", "-m", "my_plugin"]
    env:
      MY_PLUGIN_CONFIG: "/etc/sera/plugins/my-plugin.toml"
  health_check_interval: 30s
"#;

#[test]
fn grpc_manifest_parses_correctly() {
    let m = PluginManifest::from_yaml(GRPC_YAML).expect("gRPC manifest must parse");
    assert_eq!(m.kind, "Plugin");
    assert_eq!(m.metadata.name, "my-grpc-plugin");

    let reg = m.into_registration().expect("registration must succeed");
    assert_eq!(reg.name, "my-grpc-plugin");
    assert_eq!(reg.capabilities, vec![PluginCapability::ToolExecutor]);
    assert_eq!(reg.health_check_interval, Duration::from_secs(30));

    match &reg.transport {
        PluginTransport::Grpc { grpc } => {
            assert_eq!(grpc.endpoint, "localhost:9090");
            let tls = grpc.tls.as_ref().expect("TLS must be present");
            assert_eq!(tls.ca_cert, "/etc/sera/plugins/ca.crt");
            assert_eq!(tls.client_cert, "/etc/sera/plugins/client.crt");
            assert_eq!(tls.client_key, "/etc/sera/plugins/client.key");
        }
        other => panic!("expected Grpc transport, got {other:?}"),
    }
}

#[test]
fn stdio_manifest_parses_correctly() {
    let m = PluginManifest::from_yaml(STDIO_YAML).expect("stdio manifest must parse");
    assert_eq!(m.kind, "Plugin");
    assert_eq!(m.metadata.name, "my-stdio-plugin");

    let reg = m.into_registration().expect("registration must succeed");
    assert_eq!(reg.name, "my-stdio-plugin");
    assert_eq!(reg.capabilities, vec![PluginCapability::ContextEngine]);
    assert_eq!(reg.health_check_interval, Duration::from_secs(30));

    match &reg.transport {
        PluginTransport::Stdio { stdio } => {
            assert_eq!(stdio.command, vec!["/usr/bin/python", "-m", "my_plugin"]);
            assert_eq!(
                stdio.env.get("MY_PLUGIN_CONFIG").map(String::as_str),
                Some("/etc/sera/plugins/my-plugin.toml")
            );
        }
        other => panic!("expected Stdio transport, got {other:?}"),
    }
}

#[test]
fn stdio_relative_command_rejected_with_clear_error() {
    let yaml = r#"
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: bad-plugin
spec:
  capabilities: [ToolExecutor]
  transport: stdio
  stdio:
    command: ["python", "-m", "my_plugin"]
  health_check_interval: 30s
"#;
    let m = PluginManifest::from_yaml(yaml).expect("manifest must parse");
    let err = m
        .into_registration()
        .expect_err("relative command must be rejected");

    assert!(
        matches!(err, PluginError::ManifestInvalid { .. }),
        "expected ManifestInvalid, got {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("absolute path"),
        "error message must mention 'absolute path', got: {msg}"
    );
}

#[test]
fn stdio_empty_command_rejected() {
    let yaml = r#"
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: bad-plugin
spec:
  capabilities: [ToolExecutor]
  transport: stdio
  stdio:
    command: []
  health_check_interval: 30s
"#;
    let m = PluginManifest::from_yaml(yaml).expect("manifest must parse");
    let err = m
        .into_registration()
        .expect_err("empty command must be rejected");
    assert!(matches!(err, PluginError::ManifestInvalid { .. }));
}

#[test]
fn grpc_manifest_without_tls_parses() {
    let yaml = r#"
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: dev-plugin
spec:
  capabilities: [MemoryBackend]
  transport: grpc
  grpc:
    endpoint: "localhost:9091"
  health_check_interval: 30s
"#;
    let m = PluginManifest::from_yaml(yaml).unwrap();
    let reg = m.into_registration().unwrap();
    match &reg.transport {
        PluginTransport::Grpc { grpc } => {
            assert_eq!(grpc.endpoint, "localhost:9091");
            assert!(grpc.tls.is_none());
        }
        other => panic!("expected Grpc transport, got {other:?}"),
    }
}

#[test]
fn context_engine_capability_parses_in_grpc_manifest() {
    let yaml = r#"
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: ctx-grpc
spec:
  capabilities: [ContextEngine]
  transport: grpc
  grpc:
    endpoint: "ctx-service:9090"
  health_check_interval: 30s
"#;
    let reg = PluginManifest::from_yaml(yaml)
        .unwrap()
        .into_registration()
        .unwrap();
    assert_eq!(reg.capabilities, vec![PluginCapability::ContextEngine]);
}
