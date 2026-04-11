# SERA Learnings

> Non-obvious discoveries, environment gotchas, and resolved design decisions from MVS implementation.

## Build & Dependencies

### Workspace Cargo.toml had sera-runtime listed twice
**Issue**: The workspace `[workspace.dependencies]` section did NOT include sera-runtime (it wasn't a workspace dependency package), but `sera-core/Cargo.toml` listed it as a path dependency. This is correct — path dependencies live in the crate's own Cargo.toml, not the workspace.

**Resolution**: Do not add workspace-internal crates to `[workspace.dependencies]` unless they are also exposed as reusable libraries. Only do this for external crates (tokio, serde, etc.).

### SQLite uses rusqlite (sync), not sqlx (async)
**Why**: For embedded MVS use, a synchronous library is simpler and sufficient. sqlx is overkill when there's only one consumer (sera-runtime) and no connection pooling overhead.

**Trade-off**: sqlx is async-first but slower startup in embedded contexts. rusqlite has no async overhead.

**Post-MVS**: Switch to sqlx when multiple backend services need PostgreSQL with async pooling.

### sera-runtime must be both lib and bin
**Why**: `sera-core` imports reasoning loop logic from `sera-runtime` (lib), but MVS also ships a standalone `sera-runtime` binary for BYOH agent containers.

**File structure**:
```
sera-runtime/
├── src/
│   ├── lib.rs              # Public modules
│   ├── main.rs             # Binary entry point
│   ├── reasoning_loop.rs
│   └── tools/
└── Cargo.toml
```

**In Cargo.toml**:
```toml
[lib]
name = "sera_runtime"
path = "src/lib.rs"

[[bin]]
name = "sera-runtime"
path = "src/main.rs"
```

Both can coexist; no conflict.

### serde_yaml doesn't handle multi-document YAML natively
**Problem**: Parsing `---` separators in YAML requires manual splitting.

**Solution**: Split on `^---$` (regex for line-boundary `---`), then parse each document with serde_yaml separately.

```rust
let docs: Vec<&str> = yaml_text
    .split("---")
    .map(|s| s.trim())
    .filter(|s| !s.is_empty())
    .collect();

for doc_text in docs {
    let manifest: ConfigManifest = serde_yaml::from_str(doc_text)?;
    // process...
}
```

**Why not just split on `---`?** YAML can have `---` inside strings. The regex approach is safer but still a workaround.

### Secret path resolution: slashes and hyphens both become underscores
**Rule**: 
- Path: `connectors/discord-main/token`
- Env var: `SERA_SECRET_CONNECTORS_DISCORD_MAIN_TOKEN`
- Slashes → underscores
- Hyphens → underscores
- Uppercase

**Example code**:
```rust
fn env_var_name(secret_path: &str) -> String {
    format!(
        "SERA_SECRET_{}",
        secret_path
            .replace('/', "_")
            .replace('-', "_")
            .to_uppercase()
    )
}
```

This is important for shell environments where hyphens are illegal in variable names.

### Edition 2024 Rust required for the workspace
**Features used**: 
- let-chains (`if let x = y && z == w`)
- Some nightly-stabilized features in dependencies

**Minimum Rust version**: 1.81+

Set in `Cargo.toml`:
```toml
[workspace.package]
edition = "2024"
```

Older editions will fail to compile.

### tokio-tungstenite needs rustls-tls-webpki-roots feature
**Issue**: Without this feature, tokio-tungstenite uses native TLS (OpenSSL on Linux, Schannel on Windows), adding system dependencies.

**Solution**: Use rustls with webpki roots:
```toml
tokio-tungstenite = { version = "0.26", features = ["connect", "rustls-tls-webpki-roots"] }
```

**Trade-off**: Rustls adds ~500KB to binary; native TLS adds system dependency. MVS chooses bundled rustls for portability.

### The existing TS agent-runtime ContextManager uses cl100k_base tiktoken for all models
**Context**: The JavaScript `agent-runtime` in `core/agent-runtime/` uses a single encoder (OpenAI's cl100k_base) for all models.

**Accuracy**: 5-10% token count error for non-OpenAI models (acceptable for context estimation).

**In Rust**: No need to replicate; we call the LLM API which tells us actual token counts. Use those instead of estimating.

**Why?** LLM providers return `{ usage: { prompt_tokens, completion_tokens } }` in responses. Trust those numbers.

## Configuration & Secrets

### Don't store secrets in manifests
**Rule**: Never put actual secrets in sera.yaml. Always use `{ secret: "path" }` references.

**Bad**:
```yaml
spec:
  token: "sk-..."  # Exposed in repo
```

**Good**:
```yaml
spec:
  token:
    secret: "providers/openai/api-key"
    # Resolves to env var: SERA_SECRET_PROVIDERS_OPENAI_API_KEY
```

### Multi-agent governance requires careful session keying
**Issue**: If two agents share a session key, they see each other's history.

**Rule**: Session keys must include agent_id and scope:
- `agent:{agent_id}:main` for persistent
- `discord:{channel_id}` for channel-isolated

**Never**: Use globally shared session keys like `general` or `default`.

## Turn Loop & Reasoning

### Context overflow handling needs aggressive compaction
**Issue**: When context window is exceeded, simply dropping messages doesn't help—compaction must be aggressive.

**Strategy**: Keep system prompt + last 1/4 of history. This ensures:
1. Persona is preserved
2. Recent context (most recent turns) is retained
3. Old history is discarded

**Code**:
```rust
let keep = messages.len() / 4;
let keep = keep.max(2);  // At least system + 1 message
// Keep system + last `keep` messages
```

### Tool result strings must be bounded
**Issue**: A tool returning 1MB of output breaks the context window for the next LLM call.

**Solution** (post-MVS): Cap tool results at 4KB, truncate with "[Truncated: XXXX bytes]".

**In MVS**: No cap, but tool designers should be mindful.

### Error retry counts matter for user experience
**Why 2 timeout retries?** 
- First timeout is transient network blip → retry makes sense
- Second timeout indicates provider overload → give up

**Why 3 overflow retries?**
- Overflow often recoverable with compaction
- But after 3 aggressive compactions, context is too small → give up

**These are tunable**: Change constants in `reasoning_loop.rs` if UX demands.

## Storage & Sessions

### SQLite "created_at" should use UTC
**Pattern**: All timestamps should be stored as UTC ISO-8601 strings or Unix timestamps.

```rust
let now = chrono::Utc::now().to_rfc3339();
db.append_transcript(&session_id, "user", Some(message), None, None)?;
```

**Why?** Timezones are a source of bugs. UTC is unambiguous.

### Archive vs delete sessions
**Never delete** a session record.

**Rule**: Mark session state as "archived" and store `archived_at` timestamp. This preserves the audit trail.

```rust
// Session reset: don't delete old session
db.update_session_state(&old_session_id, "archived", Some(now))?;
let new_session = db.create_session(&agent_id)?;
```

## Discord & External Integrations

### Discord WebSocket heartbeat is critical
**Issue**: If heartbeat stops, Discord closes connection after ~60s.

**Rule**: Maintain steady heartbeat loop. Don't block heartbeat with long-running operations.

**Pattern**:
```rust
// Spawn separate task for heartbeat
tokio::spawn(async move {
    loop {
        tokio::time::sleep(Duration::from_millis(heartbeat_interval)).await;
        ws.send(Message::Text(heartbeat_json)).await.ok();
    }
});

// Main loop handles messages, not blocked by heartbeat
while let Some(msg) = ws_receiver.next().await {
    // process...
}
```

### Intents bitmask is binary, not additive
**Mistake**: Setting `intents: 1 + 9 + 12 + 15` (wrong).

**Correct**: Bitwise OR: `(1) | (1 << 9) | (1 << 12) | (1 << 15)` = 37377

**In config**: Provide a named constant or enum:
```rust
const INTENTS_MVS: u32 = 37377;  // GUILDS | GUILD_MESSAGES | DIRECT_MESSAGES | MESSAGE_CONTENT
```

### MESSAGE_CONTENT intent is non-negotiable for general bots
**Without it**: message.content is null for non-mentioned messages. Breaks everything.

**Include it**: Always include in intents for any bot that reads messages.

## Testing

### Use insta for YAML snapshots
**For manifests**: Assert output using `insta::assert_yaml_snapshot!` instead of manually comparing.

```rust
#[test]
fn manifest_loads_correctly() {
    let set = parse_manifests(TEMPLATE_YAML).unwrap();
    insta::assert_yaml_snapshot!(set);
}
```

This auto-generates `.snap` files and diffs on change.

### Mock the LLM client for unit tests
**Never** make real HTTP calls to LLM in unit tests.

Create a mock:
```rust
struct MockLlmClient {
    response: String,
}

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn chat(&self, _messages: &[ChatMessage], _tools: &[ToolDefinition]) 
        -> Result<LlmResponse> {
        Ok(LlmResponse {
            message: ChatMessage { content: Some(self.response.clone()), .. },
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        })
    }
}
```

## Documentation

### Link to ARCHITECTURE.md early
When implementing a feature, always check the architecture doc first. Saves relearning the crate boundaries.

### Update CLAUDE.md with discoveries
When you fix a non-obvious issue or learn a gotcha, add it to the **Learnings** section of the relevant CLAUDE.md:
- `rust/crates/sera-core/CLAUDE.md` for core module learnings
- `rust/crates/sera-runtime/CLAUDE.md` for runtime/loop learnings
- Top-level `CLAUDE.md` for cross-cutting items

Format:
```markdown
- **[Short title]**: What the issue was and what the resolution or decision is.
```

---

Last updated: 2026-04-09
