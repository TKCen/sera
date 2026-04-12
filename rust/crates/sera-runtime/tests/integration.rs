//! Integration tests for the SERA runtime.
//!
//! Tests that depended on the old reasoning_loop / context_assembler / TaskInput/TaskOutput
//! were removed in the P0-6 contract migration. New integration tests for the four-method
//! lifecycle will be added in Phase 1 when the full model integration is wired.

use serde_json::json;
use tempfile::TempDir;

use sera_runtime::manifest::{load_manifest, RuntimeManifest};
use sera_runtime::session_manager::SessionManager;
use sera_runtime::tools::mvs_tools::MvsToolRegistry;

// ---------------------------------------------------------------------------
// Test: Config loading + agent resolution (manifest)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_config_loading_and_manifest_resolution() {
    let tmp = TempDir::new().unwrap();

    // Write a sera agent manifest to temp dir
    let manifest_content = r#"
apiVersion: v1
kind: Agent
metadata:
  name: test-researcher
  namespace: default
  labels:
    tier: "1"
spec:
  identity:
    role: researcher
    bio: "A research agent for integration testing"
    instructions: "Search for information and report findings"
  model:
    name: gpt-4o
    provider: openai
    reasoning: false
  tools:
    - name: file_read
      enabled: true
    - name: shell
      enabled: true
  memory:
    enabled: true
    maxBlocks: 50
"#;

    let manifest_path = tmp.path().join("AGENT.yaml");
    std::fs::write(&manifest_path, manifest_content).unwrap();

    // Load via manifest_loader
    let manifest = load_manifest(&manifest_path).unwrap();

    // Verify agent identity
    let identity = manifest.identity();
    assert_eq!(identity.role, "researcher");
    assert!(identity.bio.contains("research agent"));
    assert!(identity.instructions.contains("Search for information"));

    // Verify model config
    let model = manifest.model();
    assert_eq!(model.name, "gpt-4o");
    assert_eq!(model.provider, "openai");
    assert!(!model.reasoning);

    // Verify tools
    let tools = manifest.tools();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].name, "file_read");
    assert!(tools[0].enabled);
    assert_eq!(tools[1].name, "shell");

    // Verify memory config
    let memory = manifest.memory();
    assert!(memory.enabled);
    assert_eq!(memory.maxBlocks, 50);

    // Verify we can use the manifest to build a system prompt
    match &manifest {
        RuntimeManifest::SpecWrapped { metadata, .. } => {
            assert_eq!(metadata.name, "test-researcher");
            assert_eq!(metadata.labels.get("tier").map(|s| s.as_str()), Some("1"));
        }
        _ => panic!("Expected SpecWrapped manifest format"),
    }
}

// ---------------------------------------------------------------------------
// Test: MvsToolRegistry definitions and basic execution
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mvs_tool_registry_integration() {
    let tmp = TempDir::new().unwrap();
    let registry = MvsToolRegistry::new(tmp.path());

    // Verify all 8 tools are registered
    let defs = registry.definitions();
    assert_eq!(defs.len(), 8);

    let names: Vec<&str> = defs
        .iter()
        .filter_map(|d| d["function"]["name"].as_str())
        .collect();
    assert!(names.contains(&"file_read"));
    assert!(names.contains(&"file_write"));
    assert!(names.contains(&"memory_read"));
    assert!(names.contains(&"memory_write"));
    assert!(names.contains(&"memory_search"));
    assert!(names.contains(&"memory_synthesize"));
    assert!(names.contains(&"shell"));
    assert!(names.contains(&"session_reset"));

    // Write a file, then read it back
    let write_result = registry
        .execute("file_write", &json!({"path": "hello.txt", "content": "world"}))
        .await
        .unwrap();
    assert!(write_result.contains("5 bytes"));

    let read_result = registry
        .execute("file_read", &json!({"path": "hello.txt"}))
        .await
        .unwrap();
    assert_eq!(read_result, "world");
}

// ---------------------------------------------------------------------------
// Test: SessionManager in-memory round trip
// ---------------------------------------------------------------------------

#[test]
fn test_session_manager_full_round_trip() {
    let sm = SessionManager::new_in_memory().unwrap();

    // Create session
    let sid = sm.get_or_create_session("round-trip-agent").unwrap();
    assert!(!sid.is_empty());

    // Same agent gets same session
    let sid2 = sm.get_or_create_session("round-trip-agent").unwrap();
    assert_eq!(sid, sid2);

    // Append messages
    use sera_runtime::types::ChatMessage;
    let user_msg = ChatMessage {
        role: "user".to_string(),
        content: Some("Hello".to_string()),
        ..Default::default()
    };
    sm.append_message(&sid, &user_msg).unwrap();

    let assistant_msg = ChatMessage {
        role: "assistant".to_string(),
        content: Some("Hi there!".to_string()),
        ..Default::default()
    };
    sm.append_message(&sid, &assistant_msg).unwrap();

    // Load transcript
    let transcript = sm.load_transcript(&sid).unwrap();
    assert_eq!(transcript.len(), 2);
    assert_eq!(transcript[0].role, "user");
    assert_eq!(transcript[1].role, "assistant");

    // Reset session
    let new_sid = sm.reset_session("round-trip-agent").unwrap();
    assert_ne!(sid, new_sid);

    // New session is empty
    let new_transcript = sm.load_transcript(&new_sid).unwrap();
    assert!(new_transcript.is_empty());

    // Old session transcript still accessible
    let old_transcript = sm.load_transcript(&sid).unwrap();
    assert_eq!(old_transcript.len(), 2);
}
