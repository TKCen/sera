# LSP Tool Interface Design — SERA Code-Introspection Tools

> **Status:** DRAFT
> **Bead:** sera-7h6
> **Epic:** 21 — ACP/IDE Bridge (code-introspection layer)
> **Author:** Writer pass, 2026-04-17
> **Depends on:** `SPEC-tools.md` §3 (Tool trait), `SPEC-interop.md` §4 (A2A adapter), `ARCHITECTURE-2.0.md`

---

## 1. Motivation

Coding agents currently use raw grep, cat, and find to understand codebases. This approach has three failure modes:

- **Hallucination of symbols.** Text search returns all occurrences of a string regardless of scope; the agent cannot distinguish a type definition from a string literal.
- **Token waste.** File-level reads force the agent to consume entire files to locate one method. A 2 000-line Rust file costs ~500 tokens to read; `get_symbols_overview` replaces it with ~40 tokens.
- **No rename/reference safety.** grep cannot distinguish callers of `foo()` from comments that mention "foo". Refactoring decisions based on grep counts are wrong.

The Language Server Protocol (LSP) solves all three problems: it understands the syntax tree and type system of the target language, returns precise symbol locations, and knows about all references. Providing LSP-backed tools to agents gives them the same navigational power that a human developer has in an IDE.

**Tradeoff accepted:** LSP servers are heavyweight processes (rust-analyzer peaks at ~1 GB RSS). The token savings and accuracy gains justify the memory cost for any session that does more than one code-navigation query.

---

## 2. Current State of `sera-tools`

The `sera-tools` crate (`rust/crates/sera-tools/src/`) today provides:

| Module | What it does |
|---|---|
| `registry.rs` | `Tool` trait (name + description only) and `ToolRegistry` HashMap |
| `sandbox/` | `SandboxProvider` trait, Docker/WASM/MicroVM adapters |
| `ssrf.rs` | `SsrfValidator` trait for URL pre-flight checks |
| `bash_ast.rs` | Tree-sitter bash analysis for command risk scoring |
| `binary_identity.rs` | SHA-256 TOFU identity for external binaries |
| `kill_switch.rs` | Agent abort signal propagation |
| `knowledge_ingest.rs` | Chunked document ingestion |
| `inference_local.rs` | Local model inference bridge |

**What is missing:**

1. The `Tool` trait in `registry.rs` exposes only `name()` and `description()`. `SPEC-tools.md` §3.1 defines a richer trait with `schema()`, `execute()`, `risk_level()`, `is_enabled()`, and `needs_approval()` — this has not yet been implemented.
2. There is no LSP client subsystem. No dependency on `tower-lsp`, `async-lsp`, or any LSP crate exists in the workspace `Cargo.toml`.
3. There is no language-server registry or process supervisor.
4. There are no code-introspection tools of any kind.

---

## 3. Tool Specifications

All four tools implement the full `Tool` trait from `SPEC-tools.md` §3.1. The serde schemas below are the JSON shapes exposed to the LLM and transmitted over MCP.

### 3.1 `get_symbols_overview`

Returns top-level symbols in a file or directory, grouped by kind (struct, fn, trait, impl, enum, module, …). Does not return bodies — only names, kinds, and byte ranges. Equivalent to the file outline panel in VS Code.

**Input schema:**

```rust
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetSymbolsOverviewInput {
    /// Relative path to a file or directory within the project root.
    pub path: String,
    /// How deep into child symbols to descend. 0 = top-level only.
    #[serde(default)]
    pub depth: u8,
}
```

**Output schema:**

```rust
#[derive(Debug, serde::Serialize)]
pub struct SymbolsOverview {
    pub path: String,
    pub language: String,
    pub symbols: Vec<SymbolEntry>,
}

#[derive(Debug, serde::Serialize)]
pub struct SymbolEntry {
    pub name: String,
    pub kind: SymbolKind,          // maps 1:1 to LSP SymbolKind integer
    pub range: ByteRange,
    pub children: Vec<SymbolEntry>, // populated when depth > 0
}

#[derive(Debug, serde::Serialize)]
pub struct ByteRange { pub start: u32, pub end: u32 }
```

**Risk level:** `RiskLevel::Read`

**Tradeoff:** Returning byte ranges rather than line numbers keeps the schema stable across editors and avoids CRLF ambiguity on Windows.

---

### 3.2 `find_symbol`

Searches for a symbol by name path pattern across the project or within a scoped path. Name path syntax: `TypeName/method_name` (slash-separated, rooted at file scope). Supports substring matching and overload indexing (`method[0]`).

**Input schema:**

```rust
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FindSymbolInput {
    /// Name path pattern, e.g. "Tool", "ToolRegistry/get", "/MyStruct/new".
    pub name_path_pattern: String,
    /// Restrict search to this file or directory. Empty = whole project.
    #[serde(default)]
    pub relative_path: String,
    /// Depth of children to return. 0 = matched symbol only.
    #[serde(default)]
    pub depth: u8,
    /// Include the full source body of the symbol.
    #[serde(default)]
    pub include_body: bool,
    /// LSP SymbolKind integers to include. Empty = all kinds.
    #[serde(default)]
    pub include_kinds: Vec<u8>,
    /// Maximum number of results. 0 = unlimited.
    #[serde(default)]
    pub max_matches: u32,
}
```

**Output schema:**

```rust
#[derive(Debug, serde::Serialize)]
pub struct FindSymbolResult {
    pub matches: Vec<SymbolMatch>,
    pub truncated: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct SymbolMatch {
    pub name_path: String,
    pub relative_path: String,
    pub range: ByteRange,
    pub kind: SymbolKind,
    pub body: Option<String>,      // only when include_body = true
    pub children: Vec<SymbolEntry>,
}
```

**Risk level:** `RiskLevel::Read`

**Tradeoff:** Substring matching is opt-in (default off) to prevent result explosion on short patterns like "get".

---

### 3.3 `find_referencing_symbols`

Finds all symbols that reference the given symbol — callers, type usages, macro invocations. Backed by LSP `textDocument/references`.

**Input schema:**

```rust
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FindReferencingSymbolsInput {
    /// Exact name path of the target symbol, e.g. "ToolRegistry/get".
    pub name_path: String,
    /// File containing the symbol. Required for disambiguation.
    pub relative_path: String,
    /// LSP SymbolKind integers to include in results. Empty = all.
    #[serde(default)]
    pub include_kinds: Vec<u8>,
}
```

**Output schema:**

```rust
#[derive(Debug, serde::Serialize)]
pub struct FindReferencingSymbolsResult {
    pub references: Vec<ReferenceMatch>,
}

#[derive(Debug, serde::Serialize)]
pub struct ReferenceMatch {
    pub referencing_symbol: SymbolMatch,
    pub snippet: String,           // ~3 lines of context around the reference
    pub reference_range: ByteRange,
}
```

**Risk level:** `RiskLevel::Read`

**Tradeoff:** Returning the enclosing symbol (not just the file+line) lets agents reason about callers structurally rather than positionally.

---

### 3.4 `query_project`

Cross-project delegation. Forwards a tool call to a named SERA project's LSP subsystem. Enables agents to ask "what calls `ToolRegistry::get` in the `sera-gateway` project?" without switching context.

**Input schema:**

```rust
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct QueryProjectInput {
    /// Registered project name, e.g. "sera-gateway".
    pub project_name: String,
    /// One of: "get_symbols_overview", "find_symbol", "find_referencing_symbols".
    pub tool_name: String,
    /// Params forwarded verbatim to the target tool. Must match that tool's input schema.
    pub params: serde_json::Value,
}
```

**Output schema:** `serde_json::Value` — the raw JSON response from the target project's tool. The gateway validates shape before forwarding to the agent.

**Risk level:** `RiskLevel::Read`

**Tradeoff:** Passing `params` as `serde_json::Value` rather than a typed enum avoids a combinatorial match on (project × tool × params-type) at the cost of runtime schema validation. Acceptable for a cross-project relay.

---

## 4. LSP Client Backend

### 4.1 Crate choice

SERA needs an **LSP client** (talking to an already-running language server), not an LSP server. The two main Rust options:

| Crate | Role | Verdict |
|---|---|---|
| `tower-lsp` | Build LSP **servers** in Rust | Wrong direction — SERA is the client |
| `async-lsp` | Build both LSP clients and servers; async, tower-based | **Preferred** for the client path |
| `lsp-types` | Pure LSP type definitions, no transport | Use as a shared type layer regardless |

**Decision:** Add `async-lsp` and `lsp-types` as dependencies in `sera-tools`. `async-lsp` provides a typed `ServerSocket` that wraps the JSON-RPC transport; SERA drives it with standard `textDocument/documentSymbol` and `workspace/symbol` requests.

**Tradeoff:** `async-lsp` is a smaller community than `tower-lsp`. The risk is API churn. Mitigation: pin to a minor version and own the version bump.

### 4.2 Process lifecycle

`LspProcessSupervisor` in `sera-tools::lsp::supervisor` spawns each language server as a child process via `tokio::process::Command` (stdin/stdout JSON-RPC pipe), sends `initialize`/waits for `initialized`, forwards requests through `LspClient::request()`, and restarts on unexpected exit (see §7).

---

## 5. Language-Server Registry

The registry maps file extensions (and optionally MIME types) to server configurations.

```rust
pub struct LspServerConfig {
    pub language_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub extensions: Vec<String>,
    pub initialization_options: serde_json::Value,
}
```

Default entries (shipped with SERA):

| Language | Extension(s) | Server command |
|---|---|---|
| Rust | `.rs` | `rust-analyzer` |
| Python | `.py` | `pyright-langserver --stdio` |
| TypeScript | `.ts`, `.tsx` | `typescript-language-server --stdio` |
| JavaScript | `.js`, `.jsx` | `typescript-language-server --stdio` |
| Go | `.go` | `gopls` |

Operators extend the registry via `sera_arch_config.json` (the project-level config already present at the workspace root):

```json
{
  "lsp_servers": [
    {
      "language_id": "kotlin",
      "command": "kotlin-language-server",
      "args": [],
      "extensions": [".kt", ".kts"],
      "initialization_options": {}
    }
  ]
}
```

**Tradeoff:** Extension-based dispatch is fast but ambiguous (`.ts` is both TypeScript and Terraform). A content-sniff fallback (shebang line, first-line magic) is deferred to Phase 3.

---

## 6. Caching Strategy

Symbol overviews are expensive: rust-analyzer performs full macro expansion before returning `workspace/symbol`. Cache results to avoid redundant LSP round-trips.

### 6.1 Cache key

`(project_root, relative_path, lsp_server_version, file_mtime)`

File mtime is checked cheaply via `std::fs::metadata` before each cache lookup. If mtime changed, evict the entry.

### 6.2 Cache store

In-process `DashMap<CacheKey, CachedSymbols>` inside `LspToolsState`. No external cache dependency. TTL: 5 minutes hard cap regardless of mtime, to guard against build systems that do not update mtime (e.g. some `touch`-less build rules).

### 6.3 Invalidation triggers

- File write detected via `FileTime::check_unchanged_since_read` (already defined in `SPEC-tools.md` §3.5).
- LSP server restart — clears all entries for that language.
- Explicit `cache_bust` flag on any input struct.

**Tradeoff:** In-process cache does not survive gateway restarts. Persisting to disk (e.g. sled) is a possible future optimization but adds complexity; skipped for Phase 1.

---

## 7. Error Handling

### 7.1 LSP server crash

If the child process exits unexpectedly:

1. Mark the `LspClient` as `Degraded`.
2. Return `ToolError::LspUnavailable { language, reason }` for in-flight requests.
3. Attempt restart up to `lsp_restart_limit` times (default: 3) with exponential backoff (1s, 4s, 16s).
4. After limit exceeded, fall back to grep (§7.3).

### 7.2 Request timeout

Each LSP request is wrapped in `tokio::time::timeout`. Default per-request timeout: 10 seconds (configurable). On timeout:

1. Log a warning with the request type and elapsed time.
2. Return `ToolError::LspTimeout`.
3. Do **not** restart the server — the server may be indexing a large workspace; a single slow request is not a crash signal.

### 7.3 Grep fallback

When LSP is unavailable or times out, the tool returns a degraded response:

```rust
pub enum SymbolSource {
    Lsp,
    GrepFallback { warning: String },
}
```

The grep fallback uses `ripgrep` (via `grep -rn` equivalent in Rust using the `ignore` crate) to find likely symbol definitions by regex heuristic (e.g. `^pub (fn|struct|enum|trait) <name>`). Results are marked `source: GrepFallback` so agents know accuracy is reduced.

**Tradeoff:** Grep fallback reintroduces the hallucination risk we set out to eliminate, but returning nothing is worse for agent usability. The `SymbolSource` discriminant lets agents decide whether to trust the result.

---

## 8. Performance Budget

Target latencies (p95, warm LSP server, local disk):

| Tool | Target p95 | Notes |
|---|---|---|
| `get_symbols_overview` (file) | 200 ms | Cache hit: < 5 ms |
| `get_symbols_overview` (dir) | 800 ms | Aggregates per-file results |
| `find_symbol` | 500 ms | `workspace/symbol` round-trip |
| `find_referencing_symbols` | 1 500 ms | `textDocument/references` + enclosing symbol lookup |
| `query_project` | adds 50 ms overhead | Network hop within localhost |

**Streaming partial results:** `get_symbols_overview` on directories with > 100 files streams file-by-file results using `async-stream`. The agent receives a `SymbolsOverview` per file as each completes rather than waiting for the full directory scan.

**When to stream:** Only directory-level `get_symbols_overview` streams. Single-file and `find_symbol` wait for the complete LSP response — partial symbol lists are more confusing than useful.

---

## 9. Security and Sandboxing

LSP servers are trusted processes. Some language servers (notably Pyright, `clangd`) can execute arbitrary project-level code via `pyproject.toml` plugins, `compile_commands.json`, or language-server extensions.

### 9.1 Sandbox tier

LSP server processes run in **Tier 2 sandbox** (network-isolated, filesystem read-only for paths outside the project root). Concretely:

- Spawned via `SandboxProvider::spawn_restricted` with the project root as the only writable mount.
- Outbound network blocked at the egress proxy layer (LSP servers do not need internet access).
- CPU and memory limits applied via cgroups: 2 CPU cores, 2 GB RSS.

### 9.2 Path traversal

All `relative_path` inputs are normalized against the project root before reaching the LSP server. `../` escapes return `ToolError::PathTraversal`.

### 9.3 Operator opt-out

Set `lsp_sandbox: false` in `sera_arch_config.json` to skip sandboxing in dev environments. A startup warning is emitted. Tradeoff: Tier 2 sandbox adds ~200 ms cold-start per LSP process (one-time per session).

---

## 10. Integration into `sera-tools::ToolDispatcher`

The four LSP tools register through the existing `ToolRegistry` once the full `Tool` trait from `SPEC-tools.md` §3.1 is implemented. New code lives in `sera-tools/src/lsp/` with sub-modules `supervisor`, `client`, `registry`, `cache`, and `tools/{get_symbols_overview,find_symbol,find_referencing_symbols,query_project}`. `LspToolsState` (supervisor + cache) is injected via `ToolContext::extensions` (`TypeMap`), avoiding a global singleton while sharing one `LspClient` per language.

---

## 11. IDE Bridge Integration

ACP was merged into A2A on 2025-08-25 (see `SPEC-interop.md` §5). The IDE bridge therefore routes through the A2A adapter in `sera-a2a`.

### 11.1 Transport

IDE plugins (VS Code, JetBrains) reach SERA via MCP over stdio (local) or SSE (remote). The `sera-mcp` bridge translates `tools/call` messages into `ToolCallRequest` and forwards to `ToolDispatcher → LspTool`.

### 11.2 A2A forwarding

Remote agents send A2A tasks; the `sera-a2a` adapter forwards them to `ToolDispatcher` via `DynamicToolCallRequest` (`SPEC-tools.md` §3.2). LSP tools need no special handling — they are first-class `ToolRegistry` entries.

---

## 12. Test Strategy

### 12.1 Mock LSP client

`sera-testing` exposes a `MockLspClient` that replays pre-recorded LSP responses from golden JSON files in `rust/crates/sera-testing/fixtures/lsp/rust_analyzer/`. Tests set up the mock, call the tool, and assert on the `SymbolsOverview` or `FindSymbolResult`.

### 12.2 Name-path parser property tests

`proptest` covers: round-trip `parse(format(p)) == p`, no panic on arbitrary Unicode, overload index non-negative integer only.

### 12.3 Integration tests

Gated behind `#[cfg(feature = "integration")]`, require `rust-analyzer` on PATH. Run against `sera-tools` itself — a self-referential test that finds the `Tool` trait and its implementors.

### 12.4 Timeout and crash tests

`tokio::test` with a mock server that delays > 10 s → assert `ToolError::LspTimeout`; exits immediately → assert restart attempted, crash counter incremented, grep fallback engaged after limit.

---

## 13. Rollout Phases

### Phase 1 — `get_symbols_overview` for Rust via rust-analyzer

**Scope:** Full `Tool` trait in `sera-tools::registry` (prerequisite); `async-lsp` + `lsp-types` deps; `LspProcessSupervisor`, `LspClient`, `LspServerRegistry` (Rust only); `GetSymbolsOverviewTool`, in-process cache, grep fallback; `MockLspClient` + golden fixture.

**Bead:** `sera-lsp-phase1`

**Success criterion:** Agent calls `get_symbols_overview("rust/crates/sera-tools/src/", 1)` and receives a structured `SymbolsOverview` without reading any file.

---

### Phase 2 — `find_symbol` + `find_referencing_symbols`

**Scope:** `FindSymbolTool` (`workspace/symbol`); `FindReferencingSymbolsTool` (`textDocument/references`); name-path parser + proptest; Python (Pyright) and TypeScript (tsserver) registry entries.

**Bead:** `sera-lsp-phase2`

**Success criterion:** Agent calls `find_referencing_symbols("Tool/execute", "rust/crates/sera-tools/src/registry.rs")` and receives all call sites.

---

### Phase 3 — Cross-project (`query_project`)

**Scope:** `QueryProjectTool` with A2A forwarding; multi-project registry keyed by root; content-sniff extension fallback; streaming directory overview; Tier 2 sandbox enforcement.

**Bead:** filed in Phase 2 completion report.

---

## 14. Open Questions

1. **rust-analyzer reload latency.** After `Cargo.toml` edits, rust-analyzer re-indexes. Poll `$/progress` until complete, or return stale-marked results immediately? Stale-marked is more responsive but may mislead.

2. **Multi-root workspaces.** 25+ crates require `linkedProjects` in `initializationOptions`. Default auto-detect may work; needs empirical validation.

3. **Python LSP server choice.** Pyright (best inference, requires Node.js) vs `jedi-language-server` (pure Python, easier to sandbox). Decision deferred to Phase 2.

4. **Symbol kind extensibility.** `rust-analyzer` emits Rust-specific `SymbolKind` integers not in the LSP spec. The `SymbolKind` enum must be `#[non_exhaustive]` to avoid deserialization failures.

5. **`query_project` credentials.** Does the forwarding agent inherit the caller's credentials, or require a service account? The A2A credential model (`SPEC-interop.md` §4) needs a concrete answer before Phase 3.

6. **Cache warming.** Pre-warm on session start (reduces first-call latency, wastes resources if unused) vs lazy on first call (simpler, slower first hit). Decision deferred to Phase 1 implementation.
