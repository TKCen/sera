//! End-to-end integration tests for the SERA MVS turn loop.
//!
//! Validates the full flow: config → agent → LLM call → tool execution → session persistence.
//! Uses an in-process mock LLM server (axum) to avoid external dependencies.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use axum::extract::State;
use axum::routing::post;
use axum::Router;
use serde_json::json;
use tempfile::TempDir;
use tokio::net::TcpListener;

use sera_runtime::config::RuntimeConfig;
use sera_runtime::context_assembler::ContextAssembler;
use sera_runtime::manifest::{load_manifest, RuntimeManifest};
use sera_runtime::reasoning_loop::{run_enhanced, LoopConfig};
use sera_runtime::session_manager::SessionManager;
use sera_runtime::tools::mvs_tools::MvsToolRegistry;
use sera_runtime::types::TaskInput;

// ---------------------------------------------------------------------------
// Mock LLM server helpers
// ---------------------------------------------------------------------------

/// Start a mock LLM server that returns a simple text response (SSE format).
/// Returns (server address, join handle).
async fn start_mock_llm_text_only() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Router::new().route("/chat/completions", post(mock_text_completion));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (addr, handle)
}

/// Mock handler: returns a simple text response in SSE streaming format.
async fn mock_text_completion() -> String {
    // Return SSE-formatted streaming response that the LlmClient can parse.
    let chunk1 = json!({
        "choices": [{
            "index": 0,
            "delta": { "content": "Hello! I'm Sera, your autonomous assistant." },
            "finish_reason": null
        }]
    });
    let chunk_done = json!({
        "choices": [],
        "usage": {
            "prompt_tokens": 50,
            "completion_tokens": 20
        }
    });
    let finish_chunk = json!({
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }]
    });

    format!(
        "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        chunk1, finish_chunk, chunk_done
    )
}

/// Shared state for the tool-call mock: tracks how many requests we've received.
#[derive(Clone)]
struct ToolCallMockState {
    call_count: Arc<AtomicU32>,
}

/// Start a mock LLM server that returns a tool call on the first request,
/// then a text response on the second.
async fn start_mock_llm_with_tool_call() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let state = ToolCallMockState {
        call_count: Arc::new(AtomicU32::new(0)),
    };

    let app = Router::new()
        .route("/chat/completions", post(mock_tool_then_text))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (addr, handle)
}

/// Mock handler: first call returns a memory_write tool call, second returns text.
async fn mock_tool_then_text(State(state): State<ToolCallMockState>) -> String {
    let n = state.call_count.fetch_add(1, Ordering::SeqCst);

    if n == 0 {
        // First call: return a tool call for memory_write
        let chunk = json!({
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_001",
                        "function": {
                            "name": "memory_write",
                            "arguments": "{\"path\":\"test_note.md\",\"content\":\"Integration test note\"}"
                        }
                    }]
                },
                "finish_reason": null
            }]
        });
        let finish = json!({
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "tool_calls"
            }]
        });
        let usage = json!({
            "choices": [],
            "usage": { "prompt_tokens": 40, "completion_tokens": 15 }
        });

        format!(
            "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            chunk, finish, usage
        )
    } else {
        // Second call: return a plain text response
        let chunk = json!({
            "choices": [{
                "index": 0,
                "delta": { "content": "Done! I wrote a note to memory." },
                "finish_reason": null
            }]
        });
        let finish = json!({
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }]
        });
        let usage = json!({
            "choices": [],
            "usage": { "prompt_tokens": 60, "completion_tokens": 12 }
        });

        format!(
            "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            chunk, finish, usage
        )
    }
}

/// Build a RuntimeConfig pointing at the given mock server address.
fn mock_config(addr: SocketAddr) -> RuntimeConfig {
    RuntimeConfig {
        llm_base_url: format!("http://{}", addr),
        llm_model: "mock-model".to_string(),
        llm_api_key: "test-key".to_string(),
        chat_port: 0,
        agent_id: "test-agent".to_string(),
        lifecycle_mode: "task".to_string(),
        core_url: "http://localhost:0".to_string(),
        api_key: "test".to_string(),
        context_window: 128_000,
        compaction_strategy: "truncate".to_string(),
        max_tokens: 4096,
    }
}

// ---------------------------------------------------------------------------
// Test: Full turn loop with mock LLM (text-only response)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_full_turn_loop_with_mock_llm() {
    // 1. Start mock LLM server on random port
    let (addr, _handle) = start_mock_llm_text_only().await;

    // 2. Create config pointing at mock
    let config = mock_config(addr);

    // 3. Create MvsToolRegistry with temp workspace
    let tmp = TempDir::new().unwrap();

    // 4. Create SessionManager with in-memory SQLite
    let sm = SessionManager::new_in_memory().unwrap();

    // 5. Run the turn: send "Hello" message
    let input = TaskInput {
        task_id: "test-turn-001".to_string(),
        prompt: "Hello".to_string(),
        context: vec![],
        agent_id: Some("test-agent".to_string()),
        session_id: None,
        max_iterations: Some(5),
    };

    let loop_config = LoopConfig {
        runtime_config: &config,
        workspace_path: Some(tmp.path()),
        persona: Some("You are a test agent."),
        memory_context: None,
        session_manager: Some(&sm),
    };

    let output = run_enhanced(loop_config, input).await.unwrap();

    // 6. Assert: response contains text
    assert_eq!(output.status, "completed");
    assert!(output.result.is_some());
    let result_text = output.result.unwrap();
    assert!(
        result_text.contains("Sera"),
        "Expected response to contain 'Sera', got: {result_text}"
    );

    // 7. Assert: session has transcript entries
    let session_id = sm.get_or_create_session("test-agent").unwrap();
    let transcript = sm.load_transcript(&session_id).unwrap();
    assert!(
        transcript.len() >= 2,
        "Expected at least 2 transcript entries (user + assistant), got: {}",
        transcript.len()
    );
    // First entry should be the user message
    assert_eq!(transcript[0].role, "user");
    assert_eq!(transcript[0].content.as_deref(), Some("Hello"));
    // Last entry should be assistant
    let last = transcript.last().unwrap();
    assert_eq!(last.role, "assistant");

    // 8. Assert: usage stats are populated
    assert!(output.usage.prompt_tokens > 0, "prompt_tokens should be > 0");
    assert!(
        output.usage.completion_tokens > 0,
        "completion_tokens should be > 0"
    );
    assert!(output.usage.iterations >= 1, "should have at least 1 iteration");
}

// ---------------------------------------------------------------------------
// Test: Turn loop with tool call
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_turn_loop_with_tool_call() {
    // 1. Start mock LLM server that returns a tool call then text
    let (addr, _handle) = start_mock_llm_with_tool_call().await;

    let config = mock_config(addr);
    let tmp = TempDir::new().unwrap();
    let sm = SessionManager::new_in_memory().unwrap();

    let input = TaskInput {
        task_id: "test-tool-001".to_string(),
        prompt: "Write a note to memory".to_string(),
        context: vec![],
        agent_id: Some("test-agent".to_string()),
        session_id: None,
        max_iterations: Some(5),
    };

    let loop_config = LoopConfig {
        runtime_config: &config,
        workspace_path: Some(tmp.path()),
        persona: Some("You are a test agent with memory tools."),
        memory_context: None,
        session_manager: Some(&sm),
    };

    let output = run_enhanced(loop_config, input).await.unwrap();

    // Verify: completed successfully
    assert_eq!(output.status, "completed");
    assert!(output.result.is_some());

    // Verify: tool was executed (memory file should exist in workspace)
    let memory_file = tmp.path().join("memory").join("test_note.md");
    assert!(
        memory_file.exists(),
        "Expected memory file to exist at: {}",
        memory_file.display()
    );
    let content = std::fs::read_to_string(&memory_file).unwrap();
    assert_eq!(content, "Integration test note");

    // Verify: tool call records exist
    assert!(
        !output.tool_calls.is_empty(),
        "Expected at least one tool call record"
    );
    assert_eq!(output.tool_calls[0].tool_name, "memory_write");

    // Verify: usage accumulated across both LLM calls
    assert!(output.usage.prompt_tokens > 0);
    assert!(output.usage.iterations >= 2, "Should have at least 2 iterations (tool call + final)");
}

// ---------------------------------------------------------------------------
// Test: Session persistence across turns
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_session_persistence_across_turns() {
    let (addr, _handle) = start_mock_llm_text_only().await;
    let config = mock_config(addr);
    let tmp = TempDir::new().unwrap();
    let sm = SessionManager::new_in_memory().unwrap();

    // Turn 1: "Remember my name is Alice"
    let input1 = TaskInput {
        task_id: "persist-001".to_string(),
        prompt: "Remember my name is Alice".to_string(),
        context: vec![],
        agent_id: Some("persist-agent".to_string()),
        session_id: None,
        max_iterations: Some(3),
    };

    let loop_config1 = LoopConfig {
        runtime_config: &config,
        workspace_path: Some(tmp.path()),
        persona: Some("You are a helpful assistant."),
        memory_context: None,
        session_manager: Some(&sm),
    };

    let output1 = run_enhanced(loop_config1, input1).await.unwrap();
    assert_eq!(output1.status, "completed");

    // Verify: transcript has entries after turn 1
    let session_id = sm.get_or_create_session("persist-agent").unwrap();
    let transcript1 = sm.load_transcript(&session_id).unwrap();
    assert!(
        transcript1.len() >= 2,
        "Turn 1 transcript should have at least 2 entries (user + assistant), got: {}",
        transcript1.len()
    );

    // Turn 2: same session — "What's my name?"
    let input2 = TaskInput {
        task_id: "persist-002".to_string(),
        prompt: "What's my name?".to_string(),
        context: vec![],
        agent_id: Some("persist-agent".to_string()),
        session_id: Some(session_id.clone()),
        max_iterations: Some(3),
    };

    let loop_config2 = LoopConfig {
        runtime_config: &config,
        workspace_path: Some(tmp.path()),
        persona: Some("You are a helpful assistant."),
        memory_context: None,
        session_manager: Some(&sm),
    };

    let output2 = run_enhanced(loop_config2, input2).await.unwrap();
    assert_eq!(output2.status, "completed");

    // Verify: transcript has grown (includes turn 1 + turn 2)
    let transcript2 = sm.load_transcript(&session_id).unwrap();
    assert!(
        transcript2.len() > transcript1.len(),
        "Turn 2 transcript ({}) should be larger than turn 1 ({})",
        transcript2.len(),
        transcript1.len()
    );

    // The history should include turn 1's user message
    assert_eq!(transcript2[0].role, "user");
    assert_eq!(
        transcript2[0].content.as_deref(),
        Some("Remember my name is Alice")
    );
}

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
// Test: ContextAssembler produces correct message ordering
// ---------------------------------------------------------------------------

#[test]
fn test_context_assembler_full_integration() {
    let persona = "You are SERA agent Alpha.";
    let tools = vec![
        json!({"type": "function", "function": {"name": "file_read", "description": "Read a file", "parameters": {}}}),
        json!({"type": "function", "function": {"name": "shell", "description": "Run command", "parameters": {}}}),
    ];
    let memory = Some("User prefers concise answers. Project is SERA.");
    let history = vec![
        json!({"role": "user", "content": "What tools do you have?"}),
        json!({"role": "assistant", "content": "I have file_read and shell."}),
    ];
    let current = "Read the config file";

    let messages = ContextAssembler::assemble(persona, &tools, memory, &history, current);

    // Expected: system(persona) + system(tools) + system(memory) + 2 history + user(current)
    assert_eq!(messages.len(), 6);

    // Verify ordering: persona first (KV cache optimization)
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], persona);

    // Tool reminder second
    assert_eq!(messages[1]["role"], "system");
    let tool_content = messages[1]["content"].as_str().unwrap();
    assert!(tool_content.contains("file_read"));
    assert!(tool_content.contains("shell"));

    // Memory third
    assert_eq!(messages[2]["role"], "system");
    assert!(messages[2]["content"].as_str().unwrap().contains("SERA"));

    // History preserved in order
    assert_eq!(messages[3]["content"], "What tools do you have?");
    assert_eq!(messages[4]["content"], "I have file_read and shell.");

    // Current message last
    assert_eq!(messages[5]["role"], "user");
    assert_eq!(messages[5]["content"], "Read the config file");
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
