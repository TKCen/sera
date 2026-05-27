//! SERA Runtime — standalone agent harness with CLI and NDJSON interfaces.
//!
//! The runtime is fully self-contained: it owns the LLM client, tool registry,
//! tool dispatch, context engine, and turn loop. No gateway required.
//!
//! Two modes:
//! - **Interactive** (default when stdin is a TTY): human-friendly chat REPL.
//! - **NDJSON** (default when stdin is piped, or `--ndjson`): machine-readable
//!   Submission/Event protocol (P0-6 `AppServerTransport::Stdio` contract —
//!   see [`sera_runtime::stdio`]).

use std::collections::HashMap;
use std::sync::Arc;

use clap::Parser;
use sera_config::CapabilityRegistry;
use sera_runtime::agent_tool_registry::{AgentToolRegistry, InProcAgentRouter};
use sera_runtime::authz_builder;
use sera_runtime::config::RuntimeConfig;
use sera_runtime::context_engine::pipeline::ContextPipeline;
use sera_runtime::default_runtime::DefaultRuntime;
use sera_runtime::health;
use sera_runtime::llm_client;
use sera_runtime::stdio;
use sera_runtime::tools::TraitToolRegistry;
use sera_runtime::tools::dispatcher::RegistryDispatcher;
use sera_types::runtime::{AgentRuntime, TurnContext, TurnOutcome};

// ── CLI ──────────────────────────────────────────────────────────────────────

/// SERA Runtime — standalone agent harness
#[derive(Parser, Debug)]
#[command(name = "sera-runtime", about = "SERA agent runtime — standalone LLM + tool execution")]
struct Cli {
    /// LLM API base URL (OpenAI-compatible)
    #[arg(long, env = "LLM_BASE_URL")]
    llm_url: Option<String>,

    /// Model name
    #[arg(long, short, env = "LLM_MODEL")]
    model: Option<String>,

    /// API key for the LLM endpoint
    #[arg(long, env = "LLM_API_KEY")]
    api_key: Option<String>,

    /// Max tokens for LLM responses
    #[arg(long, env = "MAX_TOKENS")]
    max_tokens: Option<u32>,

    /// Agent identifier
    #[arg(long, env = "AGENT_ID", default_value = "sera-local")]
    agent_id: String,

    /// System prompt prepended to every conversation
    #[arg(long, short)]
    system: Option<String>,

    /// Force NDJSON mode (even when stdin is a TTY)
    #[arg(long)]
    ndjson: bool,

    /// Disable the health check HTTP server
    #[arg(long)]
    no_health: bool,

    /// Health server port (0 = disabled)
    #[arg(long, env = "AGENT_CHAT_PORT", default_value = "0")]
    health_port: u16,
}

impl Cli {
    /// Merge CLI args over env-var defaults to produce a RuntimeConfig.
    fn into_config(self) -> RuntimeConfig {
        let mut config = RuntimeConfig::from_env();
        if let Some(url) = self.llm_url {
            config.llm_base_url = url;
        }
        if let Some(model) = self.model {
            config.llm_model = model;
        }
        if let Some(key) = self.api_key {
            config.llm_api_key = key;
        }
        if let Some(max) = self.max_tokens {
            config.max_tokens = max;
        }
        config.agent_id = self.agent_id;
        config.chat_port = if self.no_health { 0 } else { self.health_port };
        config.lifecycle_mode = "task".to_string();
        config
    }
}

// ── Authz provider construction ──────────────────────────────────────────────
//
// Construction lives in [`sera_runtime::authz_builder`] (sera-ve9x) so the
// gateway's embedded dispatch path can build the same provider without
// duplicating the role-clause parser.

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let interactive = !cli.ndjson && atty::is(atty::Stream::Stdin);
    let system_prompt = cli.system.clone();
    let config = cli.into_config();

    // NDJSON mode reserves stdout for the protocol — all tracing output
    // (including info logs) goes to stderr so it cannot corrupt the
    // Submission/Event byte stream. Interactive mode likewise writes to
    // stderr to keep stdout clean for the assistant's final response.
    let filter = if interactive {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"))
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    // Start health server in background (unless disabled)
    if config.chat_port > 0 {
        let health_port = config.chat_port;
        tokio::spawn(async move {
            if let Err(e) = health::serve(health_port).await {
                tracing::error!("Health server error: {e}");
            }
        });
    }

    let authz_provider = authz_builder::build_provider_from_config(&config);

    // sera-a1u: every runtime owns a shared DelegationBus so the three
    // delegation tools (session_spawn / session_yield / session_send) can
    // coordinate over a single subscriber registry.
    let delegation_bus = sera_runtime::delegation_bus::DelegationBus::new();
    // sera-i4en: production AgentToolRegistry backed by an InProcAgentRouter
    // so `delegate-task` / `ask-agent` / `background-task` calls reach
    // registered in-process targets instead of the always-AgentNotFound
    // placeholder. The runtime binary hosts a single agent process, so the
    // router starts empty here; the gateway's embedded transport registers
    // per-agent handlers in its own construction path.
    let agent_router: Arc<InProcAgentRouter> = Arc::new(InProcAgentRouter::new());
    let agent_registry = Arc::new(AgentToolRegistry::with_router(agent_router));
    let mut registry = TraitToolRegistry::with_builtins_and_authz(config.tool_authz_enabled)
        .with_delegation(delegation_bus)
        .with_agent_tools(agent_registry);
    if let Some(ctx) = sera_runtime::tools::skill_management::skill_management_context_from_env() {
        registry = registry.with_skill_management(ctx);
    }
    let registry = Arc::new(registry);

    // sera-eo71: load CapabilityRegistry once at startup (no hot-reload in v1
    // — operator restarts the runtime to pick up policy changes; the gateway
    // already has hot-reload via /admin/policies/reload, but the runtime
    // child does not yet observe those swaps). Bind only this single agent's
    // policy_ref (forwarded by the gateway as SERA_AGENT_POLICY_REF).
    let capability_registry = Arc::new(load_capability_registry(&config.agent_id));

    let dispatcher = RegistryDispatcher::new(Arc::clone(&registry))
        .with_capability_registry(Arc::clone(&capability_registry), config.agent_id.clone());

    // Pre-compute tool definitions for the LLM via serde round-trip, then
    // narrow to the manifest-allowed subset (bead sera-hwny). The gateway
    // forwards the agent's `tools.allow` list as `SERA_AGENT_TOOLS_ALLOW` and
    // reserves `SERA_AGENT_TOOLS_DENY` for ops/operator override; an unset or
    // empty allow list preserves the legacy "expose every built-in" behaviour.
    let tool_filter = sera_runtime::tools::filter::ToolNameFilter::from_env();
    let tool_defs: Vec<sera_types::tool::ToolDefinition> = {
        let runtime_defs = registry.definitions();
        let filtered = tool_filter.filter_definitions(runtime_defs);
        filtered
            .iter()
            .filter_map(|d| {
                let value = serde_json::to_value(d).ok()?;
                serde_json::from_value(value).ok()
            })
            .collect()
    };
    if !tool_filter.is_pass_through() {
        tracing::info!(
            tool_count = tool_defs.len(),
            "tool schema filter active (SERA_AGENT_TOOLS_ALLOW/DENY)"
        );
    }

    // sera-jvi + sera-48v: opportunistically attach an [`AccountPool`] and a
    // unified [`ThinkingConfig`] when the corresponding env vars are set.
    // Absence of either preserves the legacy single-account / no-reasoning
    // behaviour byte-for-byte.

    // Determine whether to permit turns when no ConstitutionalGate HookChain
    // is installed.  Opt-in via env var:
    //   SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE=1|true  (explicit operator opt-in)
    // The gateway forwards this env var when the operator has set it, so the
    // runtime only needs one read path.
    let permissive_gate = resolve_allow_missing_gate();

    let context_engine = Box::new(ContextPipeline::new());
    // sera-m1k8: returns either a single LlmClient or a FallbackChain wrapping
    // primary + fallback when SERA_LLM_FALLBACK_* env vars are set.
    let llm_provider = llm_client::build_provider_from_config(&config);
    let runtime = DefaultRuntime::new(context_engine)
        .with_llm(llm_provider)
        .with_tool_dispatcher(Box::new(dispatcher))
        .with_authz_provider(authz_provider)
        .with_allow_missing_constitutional_gate(permissive_gate);

    if interactive {
        run_interactive(&config, &runtime, &tool_defs, system_prompt.as_deref()).await
    } else {
        tracing::info!(
            agent_id = %config.agent_id,
            model = %config.llm_model,
            tool_count = tool_defs.len(),
            "sera-runtime starting (NDJSON transport)"
        );
        stdio::run_ndjson_loop(&config, &runtime, &tool_defs).await
    }
}

// ── Interactive REPL ─────────────────────────────────────────────────────────

async fn run_interactive(
    config: &RuntimeConfig,
    runtime: &DefaultRuntime,
    tool_defs: &[sera_types::tool::ToolDefinition],
    system_prompt: Option<&str>,
) -> anyhow::Result<()> {
    use std::io::{BufRead, Write};

    eprintln!("sera-runtime — interactive mode");
    eprintln!("  model:  {}", config.llm_model);
    eprintln!("  llm:    {}", config.llm_base_url);
    eprintln!("  tools:  {} available", tool_defs.len());
    eprintln!("  type 'exit' or Ctrl-D to quit\n");

    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut conversation: Vec<serde_json::Value> = Vec::new();

    // Add system prompt if provided
    if let Some(sys) = system_prompt {
        conversation.push(serde_json::json!({"role": "system", "content": sys}));
    }

    loop {
        // Print prompt
        eprint!("> ");
        std::io::stderr().flush()?;

        let mut input = String::new();
        let n = reader.read_line(&mut input)?;
        if n == 0 {
            // EOF (Ctrl-D)
            eprintln!();
            break;
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "exit" || trimmed == "quit" {
            break;
        }

        // Add user message to conversation
        conversation.push(serde_json::json!({"role": "user", "content": trimmed}));

        // Build TurnContext with full conversation history
        let turn_ctx = TurnContext {
            event_id: uuid::Uuid::new_v4().to_string(),
            agent_id: config.agent_id.clone(),
            session_key: format!("session:{}:interactive", config.agent_id),
            messages: conversation.clone(),
            available_tools: tool_defs.to_vec(),
            metadata: HashMap::new(),
            change_artifact: None,
            parent_session_key: None,
            tool_use_behavior: Default::default(),
        };

        let outcome = runtime.execute_turn(turn_ctx).await;

        match outcome {
            Ok(TurnOutcome::FinalOutput { response, .. }) => {
                println!("{response}\n");
                // Add assistant response to conversation history
                conversation.push(serde_json::json!({"role": "assistant", "content": response}));
            }
            Ok(TurnOutcome::Interruption { reason, .. }) => {
                eprintln!("[interrupted: {reason}]\n");
            }
            Ok(TurnOutcome::Handoff { target_agent_id, .. }) => {
                eprintln!("[handoff -> {target_agent_id}]\n");
            }
            Ok(TurnOutcome::WaitingForApproval { ticket_id, .. }) => {
                eprintln!("[waiting for approval: {ticket_id}]\n");
            }
            Ok(other) => {
                eprintln!("[{other:?}]\n");
            }
            Err(e) => {
                eprintln!("[error: {e:?}]\n");
            }
        }
    }

    Ok(())
}

// ── LLM client wiring ────────────────────────────────────────────────────────
//
// `build_llm_client` lives in [`sera_runtime::llm_client::build_from_config`]
// (sera-ve9x) so the gateway's embedded dispatch path can construct the same
// client without duplicating the env-var reads.

// ── Constitutional gate resolution ───────────────────────────────────────────

/// sera-eo71: load the CapabilityRegistry for this runtime process.
///
/// The runtime is single-agent — `agent_id` is set via `AGENT_ID` and the
/// agent's `policyRef` (if any) is forwarded by the gateway as
/// `SERA_AGENT_POLICY_REF`. The policies directory follows the same
/// resolution as the gateway: `SERA_CAPABILITY_POLICIES_DIR` overrides,
/// otherwise `ConfigRoot::policies_dir()`.
///
/// Fail-closed: a non-empty `policy_ref` that doesn't resolve to a loaded
/// policy aborts startup, mirroring the gateway-side semantics.
fn load_capability_registry(agent_id: &str) -> CapabilityRegistry {
    let policies_dir = CapabilityRegistry::resolve_policies_dir();
    let policy_ref = std::env::var("SERA_AGENT_POLICY_REF")
        .ok()
        .filter(|s| !s.is_empty());
    let bindings = vec![(agent_id.to_string(), policy_ref.clone())];
    match CapabilityRegistry::load_and_bind(&policies_dir, bindings) {
        Ok(reg) => {
            tracing::info!(
                agent_id = %agent_id,
                policy_ref = ?policy_ref,
                policies_dir = %policies_dir.display(),
                loaded_policies = reg.policy_count(),
                "Capability registry loaded (sera-eo71)"
            );
            reg
        }
        Err(e) => {
            // Fail-closed: panic so the runtime exits non-zero and the
            // gateway notices the dead child instead of silently running
            // unconstrained.
            panic!(
                "failed to initialise capability registry (sera-eo71): {e}"
            );
        }
    }
}

/// Read `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE` and return `true` when the
/// operator has explicitly opted in (value `"1"` or `"true"`, case-insensitive).
///
/// The gateway forwards this env var when the operator has set it, so the
/// runtime sees a single read path.
fn resolve_allow_missing_gate() -> bool {
    let val = std::env::var("SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false);
    if val {
        tracing::info!("constitutional gate permissive: reason=env");
    }
    val
}

/// Send periodic heartbeats to sera-core.
#[allow(dead_code)]
async fn run_heartbeat(config: &RuntimeConfig) {
    let client = reqwest::Client::new();
    let url = format!("{}/api/agents/{}/heartbeat", config.core_url, config.agent_id);

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;

        match client
            .post(&url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .json(&serde_json::json!({"status": "running"}))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                tracing::debug!("Heartbeat sent");
            }
            Ok(resp) => {
                tracing::warn!("Heartbeat returned HTTP {}", resp.status());
            }
            Err(e) => {
                tracing::warn!("Heartbeat failed: {e}");
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::resolve_allow_missing_gate;
    use std::sync::Mutex;

    // cargo test runs tests in parallel by default, so all four tests below
    // would otherwise race on SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE — one
    // setting "1", another setting "false", with assertions interleaving.
    // Holding this mutex for the whole test (env mutation + assertion)
    // serialises them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE` unset → permissive = false.
    #[test]
    fn gate_defaults_to_false_when_env_unset() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = EnvGuard::unset("SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE");
        assert!(!resolve_allow_missing_gate());
    }

    /// Value `"1"` → permissive = true (env path).
    #[test]
    fn gate_true_for_value_one() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = EnvGuard::set("SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE", "1");
        assert!(resolve_allow_missing_gate());
    }

    /// Value `"true"` (case-insensitive) → permissive = true.
    #[test]
    fn gate_true_for_value_true_case_insensitive() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = EnvGuard::set("SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE", "TRUE");
        assert!(resolve_allow_missing_gate());
    }

    /// Value `"false"` → permissive = false (not opted in).
    #[test]
    fn gate_false_for_value_false() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = EnvGuard::set("SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE", "false");
        assert!(!resolve_allow_missing_gate());
    }

    // ── RAII env-var guard ────────────────────────────────────────────────────

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prev = std::env::var(key).ok();
            // SAFETY: ENV_LOCK serialises every test that reads or writes the
            // gate env var, so no concurrent reader can observe a torn value.
            unsafe { std::env::set_var(key, value) };
            Self { key, prev }
        }

        fn unset(key: &'static str) -> Self {
            let prev = std::env::var(key).ok();
            // SAFETY: see EnvGuard::set.
            unsafe { std::env::remove_var(key) };
            Self { key, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: see EnvGuard::set.
            unsafe {
                match &self.prev {
                    Some(v) => std::env::set_var(self.key, v),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }
}
