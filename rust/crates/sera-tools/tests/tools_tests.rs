//! Integration tests for sera-tools.

use std::sync::Arc;

use sera_tools::sandbox::{
    DockerSandboxPolicy, ExecResult, FileSystemSandboxPolicy, L7Protocol, L7Rule, NetworkEndpoint,
    NetworkPolicyRule, NetworkSandboxPolicy, PolicyAction, SandboxPolicy, SandboxProvider,
};
use sera_tools::sandbox::policy::PolicyStatus;

// ---------------------------------------------------------------------------
// 1. SandboxProvider trait is object-safe
// ---------------------------------------------------------------------------
#[test]
fn sandbox_provider_trait_is_object_safe() {
    // This test passes if it compiles — Box<dyn SandboxProvider> is object-safe.
    let _check: Option<Box<dyn SandboxProvider>> = None;
}

// ---------------------------------------------------------------------------
// 2. Policy layering — 3-layer JSON roundtrip
// ---------------------------------------------------------------------------
#[test]
fn policy_layering_coarse_plus_fs_plus_network() {
    let policy = SandboxPolicy::Docker(DockerSandboxPolicy {
        filesystem: FileSystemSandboxPolicy {
            read_paths: vec!["/etc".to_string()],
            write_paths: vec!["/tmp".to_string()],
            include_workdir: true,
        },
        network: NetworkSandboxPolicy {
            rules: vec![NetworkPolicyRule {
                endpoint: NetworkEndpoint::Domain("api.example.com".to_string()),
                action: PolicyAction::Allow,
                l7_rules: vec![L7Rule {
                    protocol: L7Protocol::Https,
                    path_prefix: Some("/v1/".to_string()),
                }],
            }],
            default_deny: true,
        },
    });

    let json = serde_json::to_string(&policy).expect("serialize");
    let roundtripped: SandboxPolicy = serde_json::from_str(&json).expect("deserialize");

    // Verify the roundtrip preserved the Docker variant
    match roundtripped {
        SandboxPolicy::Docker(ref dp) => {
            assert!(dp.network.default_deny);
            assert_eq!(dp.filesystem.read_paths, vec!["/etc"]);
            assert!(dp.filesystem.include_workdir);
        }
        _ => panic!("expected Docker variant after roundtrip"),
    }
}

// ---------------------------------------------------------------------------
// 3. PolicyStatus hash changes on modification
// ---------------------------------------------------------------------------
#[test]
fn policy_status_hash_changes_on_modification() {
    use sha2::{Digest, Sha256};

    let content_a = b"policy version A";
    let content_b = b"policy version B";

    let hash_a: [u8; 32] = Sha256::digest(content_a).into();
    let hash_b: [u8; 32] = Sha256::digest(content_b).into();

    let status_a = PolicyStatus {
        version: 1,
        content_hash: hash_a,
        loaded_at: chrono::Utc::now(),
    };
    let status_b = PolicyStatus {
        version: 2,
        content_hash: hash_b,
        loaded_at: chrono::Utc::now(),
    };

    assert_ne!(status_a.content_hash, status_b.content_hash);
}

// ---------------------------------------------------------------------------
// 4-7. SSRF validator
// ---------------------------------------------------------------------------
use sera_tools::ssrf::{SsrfError, SsrfValidator};

#[test]
fn ssrf_validator_blocks_loopback() {
    let err = SsrfValidator::validate("127.0.0.1").unwrap_err();
    assert_eq!(err, SsrfError::Loopback);
}

#[test]
fn ssrf_validator_blocks_link_local() {
    let err = SsrfValidator::validate("169.254.1.1").unwrap_err();
    assert_eq!(err, SsrfError::LinkLocal);
}

#[test]
fn ssrf_validator_blocks_metadata_endpoint() {
    let err = SsrfValidator::validate("169.254.169.254").unwrap_err();
    assert_eq!(err, SsrfError::CloudMetadata);
}

#[test]
fn ssrf_validator_allows_explicit_public_ip() {
    SsrfValidator::validate("8.8.8.8").expect("public IP should be allowed");
}

// ---------------------------------------------------------------------------
// 8-9. TOFU binary identity
// ---------------------------------------------------------------------------
use sera_tools::binary_identity::{BinaryIdentity, BinaryIdentityError};

#[test]
fn tofu_binary_identity_pins_on_first_use() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("binary");
    std::fs::write(&path, b"original content").unwrap();

    let identity = BinaryIdentity::new();

    // First call should pin
    identity.verify_or_pin(&path).expect("first use should pin");

    // Tamper with the file
    std::fs::write(&path, b"tampered content").unwrap();

    // Second call should detect mismatch
    let err = identity.verify_or_pin(&path).unwrap_err();
    assert!(matches!(err, BinaryIdentityError::HashMismatch));
}

#[test]
fn tofu_binary_identity_accepts_unchanged_binary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stable_binary");
    std::fs::write(&path, b"stable content").unwrap();

    let identity = BinaryIdentity::new();
    identity.verify_or_pin(&path).expect("first use");
    identity.verify_or_pin(&path).expect("second use should still pass");
}

// ---------------------------------------------------------------------------
// 10. Kill switch boot health check fails closed
// ---------------------------------------------------------------------------
use sera_tools::kill_switch::boot_health_check;

#[test]
fn kill_switch_boot_health_check_fails_closed() {
    // Bad path (non-existent parent directory) should return Err, not panic
    let result = boot_health_check("/nonexistent/path/to/socket.sock");
    assert!(result.is_err(), "bad path should fail");
}

// ---------------------------------------------------------------------------
// 11. InferenceLocal URL rewriter
// ---------------------------------------------------------------------------
use sera_tools::inference_local::InferenceLocalResolver;

#[test]
fn inference_local_rewrites_url() {
    let resolver = InferenceLocalResolver::new("http://gateway.internal:8080");
    let rewritten = resolver.rewrite("http://inference.local/v1/chat");
    assert_eq!(rewritten, "http://gateway.internal:8080/v1/chat");
}

// ---------------------------------------------------------------------------
// 12. Bash AST checker blocks backtick substitution
// ---------------------------------------------------------------------------
use sera_tools::bash_ast::{BashAstChecker, BashAstError};

#[test]
fn bash_ast_checker_blocks_backtick_substitution() {
    let err = BashAstChecker::check("ls `id`").unwrap_err();
    assert_eq!(err, BashAstError::BacktickSubstitution);
}

// ---------------------------------------------------------------------------
// 13. Queue mode / sandbox policy serde roundtrip
// ---------------------------------------------------------------------------
#[test]
fn queue_mode_serde_roundtrip() {
    let policy = SandboxPolicy::Wasm;
    let json = serde_json::to_string(&policy).unwrap();
    let back: SandboxPolicy = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, SandboxPolicy::Wasm));

    let policy2 = SandboxPolicy::None;
    let json2 = serde_json::to_string(&policy2).unwrap();
    let back2: SandboxPolicy = serde_json::from_str(&json2).unwrap();
    assert!(matches!(back2, SandboxPolicy::None));
}

// ---------------------------------------------------------------------------
// 14. Tool registry register and get
// ---------------------------------------------------------------------------
use sera_tools::registry::{Tool, ToolRegistry};

struct EchoTool;

impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> &str {
        "Echoes input back"
    }
}

#[test]
fn tool_registry_register_and_get() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));

    let tool = registry.get("echo").expect("tool should be registered");
    assert_eq!(tool.name(), "echo");
    assert_eq!(tool.description(), "Echoes input back");

    assert!(registry.get("nonexistent").is_none());

    let names = registry.list();
    assert_eq!(names, vec!["echo"]);
}

// ---------------------------------------------------------------------------
// 15. ExecResult captures output fields correctly
// ---------------------------------------------------------------------------
#[test]
fn exec_result_captures_output() {
    let result = ExecResult {
        exit_code: 42,
        stdout: "hello stdout".to_string(),
        stderr: "hello stderr".to_string(),
    };

    assert_eq!(result.exit_code, 42);
    assert_eq!(result.stdout, "hello stdout");
    assert_eq!(result.stderr, "hello stderr");
}
