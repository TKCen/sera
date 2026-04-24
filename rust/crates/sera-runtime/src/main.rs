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
use sera_auth::authz::{ActionKind, AuthzProviderAdapter, RoleBasedAuthzProvider};
use sera_runtime::config::RuntimeConfig;
use sera_runtime::context_engine::pipeline::ContextPipeline;
use sera_runtime::default_runtime::DefaultRuntime;
use sera_runtime::health;
use sera_runtime::llm_client::LlmClient;
use sera_runtime::stdio;
use sera_runtime::tools::TraitToolRegistry;
use sera_runtime::tools::dispatcher::RegistryDispatcher;
use sera_types::principal::PrincipalId;
use sera_types::runtime::{AgentRuntime, TurnContext, TurnOutcome};
use sera_types::tool::AuthzProviderHandle;

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

/// Build an [`AuthzProviderHandle`] from config.
///
/// When `config.tool_authz_roles` is set, parses a compact role definition
/// string and constructs a [`RoleBasedAuthzProvider`] wrapped in an
/// [`AuthzProviderAdapter`]. Format:
///
/// ```text
/// <role>:<kind>[,<kind>...][;<role>:...]
/// ```
///
/// Supported `<kind>` names (case-insensitive): `read`, `write`, `execute`,
/// `admin`, `tool_call`, `session_op`, `memory_access`, `config_change`,
/// `propose_change`, `approve_change`.
///
/// Principal assignments are not parsed here — they are set per-agent via
/// `TOOL_AUTHZ_PRINCIPALS` (future bead). For now the provider is useful for
/// role-grant inspection and tests that inject principals directly.
///
/// When `tool_authz_roles` is `None`, returns the allow-all default stub.
fn build_authz_provider(config: &RuntimeConfig) -> Arc<dyn AuthzProviderHandle> {
    let Some(roles_str) = &config.tool_authz_roles else {
        return Arc::new(sera_types::tool::DefaultAuthzProviderStub);
    };

    let mut builder = RoleBasedAuthzProvider::builder();

    for role_clause in roles_str.split(';') {
        let role_clause = role_clause.trim();
        if role_clause.is_empty() {
            continue;
        }
        let Some((role, kinds_str)) = role_clause.split_once(':') else {
            tracing::warn!(
                "TOOL_AUTHZ_ROLES: skipping malformed clause (no ':'): {role_clause}"
            );
            continue;
        };
        let role = role.trim();
        let kinds: Vec<ActionKind> = kinds_str
            .split(',')
            .filter_map(|k| parse_action_kind(k.trim()))
            .collect();
        builder = builder.grant(role, kinds);
    }

    // Assign the runtime's own agent-id as a full-access principal so the
    // default single-agent deployment works without additional config.
    if let Ok(agent_id) = std::env::var("AGENT_ID")
        && !agent_id.is_empty()
    {
        builder = builder.assign(
            PrincipalId::new(format!("agent:{agent_id}")),
            ["operator"],
        );
    }

    Arc::new(AuthzProviderAdapter::new(builder.build()))
}

/// Parse a single action-kind name (case-insensitive).
fn parse_action_kind(s: &str) -> Option<ActionKind> {
    match s.to_ascii_lowercase().as_str() {
        "read" => Some(ActionKind::Read),
        "write" => Some(ActionKind::Write),
        "execute" => Some(ActionKind::Execute),
        "admin" => Some(ActionKind::Admin),
        "tool_call" => Some(ActionKind::ToolCall),
        "session_op" => Some(ActionKind::SessionOp),
        "memory_access" => Some(ActionKind::MemoryAccess),
        "config_change" => Some(ActionKind::ConfigChange),
        "propose_change" => Some(ActionKind::ProposeChange),
        "approve_change" => Some(ActionKind::ApproveChange),
        other => {
            tracing::warn!("TOOL_AUTHZ_ROLES: unknown action kind '{other}', skipping");
            None
        }
    }
}

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

    let authz_provider = build_authz_provider(&config);

    // sera-a1u: every runtime owns a shared DelegationBus so the three
    // delegation tools (session_spawn / session_yield / session_send) can
    // coordinate over a single subscriber registry.
    let delegation_bus = sera_runtime::delegation_bus::DelegationBus::new();
    let registry = TraitToolRegistry::with_builtins_and_authz(config.tool_authz_enabled)
        .with_delegation(delegation_bus);
    let registry = Arc::new(registry);
    let dispatcher = RegistryDispatcher::new(Arc::clone(&registry));

    // Pre-compute tool definitions for the LLM via serde round-trip
    let tool_defs: Vec<sera_types::tool::ToolDefinition> = registry
        .definitions()
        .iter()
        .filter_map(|d| {
            let value = serde_json::to_value(d).ok()?;
            serde_json::from_value(value).ok()
        })
        .collect();

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
    let llm_client = Box::new(build_llm_client(&config));
    let runtime = DefaultRuntime::new(context_engine)
        .with_llm(llm_client)
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

/// Build an [`LlmClient`] with optional sera-jvi account pool + sera-48v
/// thinking config attached.
///
/// The runtime stays fully backwards-compatible: when `SERA_<PROVIDER>_KEYS`
/// is not set for the inferred provider id, no pool is attached and the
/// client falls back to the single-account `LLM_BASE_URL` / `LLM_API_KEY`
/// path.  Likewise `SERA_REASONING_LEVEL` defaults to `off` when unset.
fn build_llm_client(config: &RuntimeConfig) -> LlmClient {
    use sera_config::providers::ProviderAccountsConfig;
    use sera_models::{
        AccountPool, CooldownConfig, ProviderAccount, ProviderKind, ReasoningLevel, ThinkingConfig,
    };

    // Provider kind is inferred from LLM_MODEL (e.g. "gpt-4o" → OpenAI,
    // "claude-3-5-sonnet" → Anthropic).  Operators can also set
    // SERA_LLM_PROVIDER_ID to pin the inference explicitly.
    let provider_id = std::env::var("SERA_LLM_PROVIDER_ID")
        .unwrap_or_else(|_| config.llm_model.clone());
    let provider_kind = ProviderKind::infer(&provider_id);

    // Thinking / reasoning level.
    let level = std::env::var("SERA_REASONING_LEVEL")
        .ok()
        .and_then(|v| match v.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "" => Some(ReasoningLevel::Off),
            "low" => Some(ReasoningLevel::Low),
            "medium" | "med" => Some(ReasoningLevel::Medium),
            "high" => Some(ReasoningLevel::High),
            _ => None,
        })
        .unwrap_or(ReasoningLevel::Off);
    let budget = std::env::var("SERA_REASONING_BUDGET_TOKENS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok());
    let mut thinking = ThinkingConfig::new(level);
    thinking.budget_tokens = budget;

    let mut client = LlmClient::new(config)
        .with_thinking(thinking)
        .with_provider_kind(provider_kind);

    // Account pool (sera-jvi).  Only attached when at least one key is
    // configured for the active provider id.
    let accounts_cfg = ProviderAccountsConfig::from_env();
    if let Some(keys) = accounts_cfg.keys_for(&provider_id)
        && !keys.is_empty()
    {
        let accounts: Vec<ProviderAccount> = keys
            .iter()
            .enumerate()
            .map(|(idx, key)| ProviderAccount::new(format!("{provider_id}-{idx}"), key.clone(), None))
            .collect();
        let pool = Arc::new(
            AccountPool::new(provider_id.clone(), accounts, CooldownConfig::default())
                .with_default_base_url(config.llm_base_url.clone()),
        );
        tracing::info!(
            provider = %provider_id,
            account_count = keys.len(),
            "Attached LLM account pool (sera-jvi)"
        );
        client = client.with_account_pool(pool);
    }

    client
}

// ── Constitutional gate resolution ───────────────────────────────────────────

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

    /// `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE` unset → permissive = false.
    #[test]
    fn gate_defaults_to_false_when_env_unset() {
        // Guard: only run when the env var is not already set by the caller.
        if std::env::var("SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE").is_ok() {
            return;
        }
        assert!(!resolve_allow_missing_gate());
    }

    /// Value `"1"` → permissive = true (env path).
    #[test]
    fn gate_true_for_value_one() {
        // Use a scoped env helper to avoid leaking between parallel tests.
        let _guard = EnvGuard::set("SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE", "1");
        assert!(resolve_allow_missing_gate());
    }

    /// Value `"true"` (case-insensitive) → permissive = true.
    #[test]
    fn gate_true_for_value_true_case_insensitive() {
        let _guard = EnvGuard::set("SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE", "TRUE");
        assert!(resolve_allow_missing_gate());
    }

    /// Value `"false"` → permissive = false (not opted in).
    #[test]
    fn gate_false_for_value_false() {
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
            // SAFETY: tests run single-threaded (no other threads read this var).
            unsafe { std::env::set_var(key, value) };
            Self { key, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: tests run single-threaded (no other threads read this var).
            unsafe {
                match &self.prev {
                    Some(v) => std::env::set_var(self.key, v),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }
}
