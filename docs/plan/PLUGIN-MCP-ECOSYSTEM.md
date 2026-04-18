# Plugin / MCP Ecosystem — Design & Scoping Doc

> **Bead:** sera-1vg  
> **Status:** DRAFT  
> **Date:** 2026-04-17  
> **Covers:** GH#148-150, GH#328-332 — plugin spec, OCI distribution, skill search,
> MCP management, sera doctor

---

## 1. Context

SERA's extension story has five interdependent pieces that allow SERA to be
extended, distributed, discovered, and diagnosed without forking the core binary.

| Piece | What it enables |
|---|---|
| **Plugin spec** | Out-of-process extensions in any language via gRPC |
| **OCI distribution** | Package and pull plugins from any container registry |
| **Skill search** | Agents discover capability packs from FS and registries |
| **MCP management** | Runtime connect/disconnect of external tool sources |
| **sera doctor** | Operator self-service diagnostics — "is my setup correct?" |

These five pieces are currently in different stages: `sera-plugins` is scaffolded
with real data types, `sera-mcp` has trait interfaces, `sera-skills` has a
filesystem loader, and OCI distribution and `sera doctor` are absent entirely.
The cross-cutting concern that ties all five together is **trust** — every extension
point that accepts external artifacts or external connections must enforce
authentication, capability attestation, and audit.

---

## 2. Topic 1 — Plugin Spec

### 2.1 Current State

`rust/crates/sera-plugins/` is **scaffolded with working data structures**. The
following modules exist and compile:

- `src/types.rs` — `PluginCapability` (6 variants + `Custom(String)`),
  `PluginRegistration`, `PluginVersion`, `PluginHealth`, `PluginInfo`, `TlsConfig`
- `src/manifest.rs` — YAML `kind: Plugin` parsing, `PluginManifest →
  PluginRegistration` conversion, duration parsing (`30s`, `5m`, `1h`)
- `src/registry.rs` — `PluginRegistry` trait + `InMemoryPluginRegistry`
  (`RwLock<HashMap>`)
- `src/circuit_breaker.rs` — three-state circuit breaker for failure isolation
- `src/error.rs` — `PluginError` with `From<PluginError> for SeraError`

What is **not** present:

- gRPC transport layer — no tonic stubs, no `.proto` files in `rust/proto/`
- Gateway wiring — `rust/crates/sera-gateway/src/plugin/mod.rs` exists as a
  directory but its contents are unexplored stubs
- Capability token signing — no signing or verification logic
- Persistent plugin registry — in-memory only, lost on gateway restart
- Hot-registration persistence — SPEC-plugins §3 says target is "yes" but
  semantics are undesigned

### 2.2 Proposed Architecture

```
sera.d/plugins/my-plugin.yaml   (Kind: Plugin manifest)
        │
        ▼
PluginManifest::from_yaml()  →  PluginRegistration
        │
        ▼
sera-gateway/plugin/loader.rs   (scans sera.d/ at startup)
        │                        (watches for new files at runtime)
        ▼
InMemoryPluginRegistry          (add DbPluginRegistry for persistence)
        │
        ▼
PluginDispatcher                (routes "backend: plugin:X" selections)
        │
        ▼
tonic gRPC channel  ──────────►  External plugin process
```

Proto files live at `rust/proto/plugin/` (`registry.proto`, `tool_executor.proto`,
`memory_backend.proto`). Apache-2.0 — third parties generate stubs with standard `protoc`.

**Capability token:** Gateway signs a short-lived JWT `{plugin_name, capabilities[],
issued_at, expiry}` using the admin key from `sera-secrets`. Plugin echoes it on
every heartbeat. Requires admin key at gateway startup — acceptable for Tier 2/3.

### 2.3 Integration Points

- `sera-gateway` — plugin registry is a gateway-owned service; plugins cannot
  bypass gateway AuthZ
- `sera-tools` — `ToolExecutor` plugins register into `sera-tools`' tool registry;
  same `pre_tool` hook chain applies
- `sera-secrets` — capability token signing key lives in the secrets store
- `sera-errors` — `PluginError` already implements `From<PluginError> for SeraError`

### 2.4 Tradeoffs

- gRPC over REST: type-safe codegen vs. heavier tonic/prost dependency
- InMemory registry: fast start vs. lost registrations on restart (add `DbPluginRegistry` in phase M)
- Short-lived capability JWT: low revocation latency vs. plugin must implement refresh
- `Custom(String)` capability: open extension vs. gateway cannot validate the proto contract

### 2.5 Phases

- **S** — `.proto` files; tonic stubs; wire `sera-gateway/plugin/` to `InMemoryPluginRegistry`
- **M** — `DbPluginRegistry`; capability token signing; hot-registration; `sera.d/` watcher
- **L** — Full contract validation at registration; `sera-plugin-sdk` crate; multi-language examples

---

## 3. Topic 2 — OCI Distribution

### 3.1 Current State

**Absent.** No OCI packaging, no ORAS usage, no registry client. The concept is
not mentioned in any Rust crate. `SPEC-plugins.md` §10 lists "plugin marketplace"
as out of scope for 1.0 but says nothing about OCI.

### 3.2 Proposed Architecture

OCI artifacts follow the ORAS (OCI Registry As Storage) pattern used by Helm OCI
charts and WASM component distributions. A SERA plugin OCI artifact contains:

```
OCI manifest
  └── layer: plugin binary or container image reference (application/vnd.sera.plugin.binary)
  └── layer: plugin manifest YAML                       (application/vnd.sera.plugin.manifest.v1)
  └── layer: proto schema bundle (optional)             (application/vnd.sera.plugin.proto)
```

**Publish flow** (`sera plugin push`):
1. `sera build` (or user's own toolchain) produces a plugin binary
2. `sera plugin push ghcr.io/org/my-plugin:1.0.0` bundles the binary +
   `plugin.yaml` manifest as an OCI artifact and pushes via ORAS protocol

**Pull flow** (`sera plugin pull`):
1. Resolves the reference against the configured registry
2. Verifies the image signature (cosign / sigstore)
3. Extracts the plugin manifest and binary to `~/.sera/plugins/`
4. Optionally auto-registers with a running gateway

**Cargo dependency:** `oci-distribution` crate (Apache-2.0) or raw `reqwest` against
the OCI Distribution Spec v1.1 API. The `oci-distribution` crate is lighter and
already used in the WASM ecosystem.

**Tradeoff:** Cosign verification adds a runtime dependency on the sigstore TUF
root — acceptable for production, but complicates air-gapped deployments. Mitigation:
make signature verification opt-in via `--verify-signature` flag, default off in
development tiers.

### 3.3 Integration Points

- New `sera-oci` crate (or module in `sera-plugins`) — pull/push logic
- `sera-cli` (Go) or a new `sera` Rust binary — surface as `sera plugin push/pull`
- `sera-secrets` — registry credentials (token auth, docker config.json)
- Plugin manifest YAML (Topic 1) — must be valid before push is accepted

### 3.4 Tradeoffs

- OCI vs. custom registry: reuses GHCR/ECR/Harbor vs. OCI Distribution Spec complexity
- Cosign: strong provenance vs. TUF root internet dependency (make opt-in via `--verify-signature`)
- Binary-in-OCI vs. image reference: simpler for compiled plugins vs. no support for Python/JS plugins
- `sera-oci` crate vs. module in `sera-plugins`: cleaner boundary vs. extra crate overhead

### 3.5 Phases

- **S** — OCI pull for plugin manifest YAML only; no signing
- **M** — OCI push + pull for binaries; registry auth via docker config.json
- **L** — Cosign verification; air-gapped mirror support; `sera plugin search` against OCI index

---

## 4. Topic 3 — Skill Search

### 4.1 Current State

`rust/crates/sera-skills/` is **scaffolded with a working filesystem loader**.
Present modules:

- `src/loader.rs` — `SkillLoader` (filesystem) + `FileSystemSkillPack`; scans a
  base directory for skill pack subdirectories; loads `{name}.json` skill
  definitions and `{name}.yaml` configs
- `src/skill_pack.rs` — `SkillPack` trait (list, get_skill, get_config, load_bundle)
- `src/bundle.rs` — `SkillBundle` (loaded collection of skills)
- `src/knowledge_schema.rs` — `KnowledgeSchemaValidator`, `SchemaViolation`
- `src/knowledge_activity_log.rs` — `KnowledgeActivityLog`, `KnowledgeOp`
- `src/knowledge_lint.rs` — `BasicLinter`, `LintReport`, `LintFinding`

What is **not** present:

- Registry-based discovery — no remote index, no OCI pull for skill packs
- Search / filtering — no fuzzy search, no tag-based query
- Agent-side skill selection — no mechanism for an agent to request skills at
  runtime vs. at startup
- Skill versioning — `FileSystemSkillPack` hardcodes `"1.0.0"` in
  `SkillBundle::new()`

### 4.2 Proposed Architecture

```
Agent manifest (mcp_servers / skills: [...])
        │
        ▼
SkillResolver (new)
        ├── FileSystemSource   →  SkillLoader (existing)
        ├── PluginSource       →  ToolExecutor plugins advertising skills
        └── RegistrySource     →  OCI index pull (depends on Topic 2)
        │
        ▼
SkillBundle (deduplicated, version-resolved)
        │
        ▼
sera-runtime context injection
```

`SkillResolver` accepts a list of `SkillRef` (name, optional version constraint) and
returns a merged `SkillBundle`. Resolution order: filesystem → plugin-advertised →
remote registry. First match wins; version constraints use semver.

**Tradeoff:** Lazy resolution (load on first agent request) vs. eager resolution
(load all skills at gateway startup). Eager is simpler but slow for large registries.
Recommended: eager for filesystem sources, lazy for registry sources.

### 4.3 Integration Points

- `sera-runtime` — injects resolved `SkillBundle` into agent context
- `sera-plugins` — `ToolExecutor` plugins can advertise skill manifests alongside tools
- OCI distribution (Topic 2) — `RegistrySource` depends on `sera-oci` pull logic
- Agent YAML manifest — `skills:` block references `SkillRef` entries

### 4.4 Tradeoffs

- First-match resolution: predictable vs. operator must know override precedence (FS > plugin > registry)
- Semver constraints in `SkillRef`: standard tooling vs. complexity for simple single-pack deployments
- Merge vs. replace on conflict: merge gives more tools vs. unpredictable capability composition

### 4.5 Phases

- **S** — `SkillResolver` with filesystem source; `sera skills list`
- **M** — Plugin-advertised skills; semver constraints; `sera skills search <query>`
- **L** — Registry source via OCI (depends on Topic 2); skill pack lock file

---

## 5. Topic 4 — MCP Management

### 5.1 Current State

`rust/crates/sera-mcp/` has **trait interfaces only** — no implementations:

- `McpServer` trait — `list_tools`, `call_tool`
- `McpClientBridge` trait — `connect`, `disconnect`, `list_tools`,
  `list_server_tools`, `call_tool`
- `McpServerConfig` — per-agent config struct (name, url, command, args, transport,
  env)
- `McpTransport` enum — `Stdio | Sse | StreamableHttp`
- `McpError` with `From<McpError> for SeraError`

The `rmcp ^1.3` dependency (Anthropic official Rust SDK) is listed in `SPEC-interop`
as the intended dependency but is not yet in `Cargo.toml`. No UI or CLI surface for
MCP management exists.

### 5.2 Proposed Architecture

**Runtime management** means operators and agents can connect/disconnect MCP servers
without restarting SERA. The management surface is:

```
sera mcp list                          # all connected servers + tool counts
sera mcp connect --name github \
    --url http://localhost:3000 \
    --transport sse                    # runtime connect
sera mcp disconnect github             # runtime disconnect
sera mcp tools [--server github]       # list tools, optionally filtered
```

Gateway REST endpoints (`/api/mcp/servers`, `/api/mcp/servers/{name}`) expose the
same operations for the `sera-web` dashboard and programmatic clients.

**Implementation path:** Add `rmcp ^1.3` to workspace → implement `RmcpClientBridge`
and `RmcpServer` in `sera-mcp` → wire into `sera-gateway/src/connector/` → REST
routes in `sera-gateway/src/routes/` → surface in `sera-tui` and CLI.

Per-agent MCP config is the default (per current spec); global pool is a later
option. SPEC-interop §10 Q1 leaves this open — recommendation: per-agent first.

### 5.3 Integration Points

- `rmcp ^1.3` — concrete transport implementations
- `sera-gateway/src/connector/` — MCP bridge connects here
- `sera-tools` — MCP server exposes the tool registry via MCP protocol
- `sera-auth` — inbound MCP callers authenticate against `sera-auth`
- `sera-tui` — new MCP management view
- Agent YAML manifest — `mcp_servers:` block (already specced)

### 5.4 Tradeoffs

- Per-agent config vs. global pool: isolation vs. operational simplicity (SPEC-interop §10 Q1 unresolved)
- `rmcp` crate vs. hand-rolled: less code vs. dependency on external release cadence
- stdio servers as subprocesses: simple for local tools vs. orphan-process lifecycle complexity
- REST management API vs. CLI only: dashboard-accessible vs. simpler implementation scope

### 5.5 Phases

- **S** — `RmcpClientBridge` implementing `McpClientBridge`; `sera mcp list/connect/disconnect`
- **M** — REST management API; `sera-web` dashboard integration; per-agent namespace enforcement
- **L** — Global MCP server pool; stdio server restart/health; tool result caching

---

## 6. Topic 5 — sera doctor

### 6.1 Current State

**Absent.** No `doctor` subcommand, no diagnostic crate, no references to a
diagnostic pattern anywhere in the Rust workspace or CLI. The Go CLI in `cli/`
has auth-flow commands but no diagnostic commands.

### 6.2 Proposed Architecture

`sera doctor` is a self-contained diagnostic runner that checks SERA's setup and
reports pass/fail for each check with an actionable remediation message.

```
sera doctor [--format text|json] [--fix]

  [✓] Database reachable
  [✓] JWT secret configured
  [✗] LiteLLM: no models available — set LITELLM_API_KEY
  [✗] MCP: github server unreachable (connection refused localhost:3000)
  [✓] Skill packs: 2 loaded

2 checks failed. Run `sera doctor --fix` to attempt remediation.
```

**Check categories:**

| Category | Checks |
|---|---|
| **Core** | DB reachable, migrations current, JWT secret set |
| **Providers** | LiteLLM reachable, at least one model available |
| **Plugins** | Registry healthy, registered plugin health |
| **MCP** | Each configured MCP server reachable |
| **Skills** | Skill base path exists, at least one pack loadable |
| **Secrets** | Required env vars present (no values leaked to output) |
| **Networking** | Egress proxy reachable (if configured), Centrifugo reachable |

**Implementation:** New `sera-doctor` crate. Each check implements a `DoctorCheck`
trait (`name`, `category`, `run(&ctx) -> CheckResult`, `fix(&ctx) -> FixResult`).
`DoctorContext` holds `SeraConfig`, DB pool, and HTTP client. Independent checks
run in parallel; ordered checks (e.g. DB before migrations) run sequentially.
`--fix` is limited to safe mutations: create missing directories, scaffold config
snippets. Never auto-rotate secrets or restart services.

### 6.3 Integration Points

- `sera-config` — `DoctorContext` loads config from the same path as the gateway
- `sera-db` — DB reachability and migration check
- `sera-plugins` — plugin health checks via `InMemoryPluginRegistry::list()`
- `sera-mcp` — MCP server reachability
- `sera-skills` — skill pack loadability
- Go CLI or new Rust `sera` binary — surface as `sera doctor` subcommand

### 6.4 Tradeoffs

- Separate `sera-doctor` crate vs. gateway module: standalone usability vs. extra crate overhead
- `--fix` scope: reduce friction vs. risk of unexpected mutations (limit to directory creation + config scaffolding)
- Parallel check execution: faster vs. interleaved output; mitigate by buffering and rendering in order
- JSON output: CI-parseable vs. added formatting work

### 6.5 Phases

- **S** — Core + provider checks; text output; standalone binary
- **M** — Plugin, MCP, skills, secrets checks; JSON output; `--fix` for missing dirs
- **L** — Networking checks (egress proxy, Centrifugo); CI exit codes; `--fix` config scaffolding

---

## 7. Cross-Cutting Concerns

### 7.1 Trust and Signing

All five topics touch trust at the boundary:

- Plugin manifests loaded from `sera.d/` are trusted by filesystem ACL; plugins
  pulled via OCI must be signature-verified (cosign, opt-in)
- Capability tokens signed by the gateway admin key gate plugin registration
- MCP servers connected at runtime require the connecting operator to have
  `mcp:connect` SERA permission
- Skill packs pulled from OCI follow the same signing path as OCI plugins
- `sera doctor` output must never log secret values — only presence/absence

### 7.2 Versioning and API Compat

- Plugin proto contracts are versioned in the proto package name
  (`sera.plugin.v1`). Breaking changes require a new package version and a
  deprecation window of at least one SERA minor release.
- Skill pack format (`SkillDefinition` JSON schema) is versioned in the schema
  itself. The `KnowledgeSchemaValidator` in `sera-skills/src/knowledge_schema.rs`
  must enforce the version field.
- MCP protocol versioning is delegated to `rmcp` — SERA tracks `rmcp ^1.3` and
  upgrades conservatively.

### 7.3 Audit

Every plugin invocation, MCP tool call, and skill load event produces an audit
entry via `sera-events`. Enforcement is at the gateway dispatch layer.

---

## 8. Rollout Order

Dependencies flow bottom-up:

1. **Phase 1 — Plugin spec**: proto files → tonic stubs → gateway wiring → capability token signing. Ground truth for all extension artifacts.
2. **Phase 2 — OCI push/pull** (needs Phase 1 schema): `sera-oci` crate → `sera plugin push/pull` → cosign opt-in. Unblocks Phase 5.
3. **Phase 3 — sera doctor** (parallel with Phase 2): core/provider checks first; plugin + MCP checks after Phases 1 and 4 land.
4. **Phase 4 — MCP management** (independent of OCI): `rmcp` → `RmcpClientBridge` → REST API → TUI view.
5. **Phase 5 — Skill search with registry** (needs Phase 2): `SkillResolver` `RegistrySource` → `sera skills search`.

---

## 9. Follow-Up Beads

Filed via `bd create` after this doc is committed:

- **sera-rlsh** (already filed) — `sera-ecosystem-phase1`: plugin manifest schema + gRPC stubs
- **phase2 bead** — `sera-ecosystem-phase2`: OCI publish/pull for plugins and skills
- **phase3 bead** — `sera-ecosystem-phase3`: sera doctor CLI
- **phase4 bead** — `sera-ecosystem-phase4`: MCP runtime management
- **phase5 bead** — `sera-ecosystem-phase5`: skill search with registry source

---

## 10. Open Design Questions

1. **Per-agent vs. global MCP server pool** (SPEC-interop §10 Q1): Should each
   agent own its MCP connections or should SERA maintain a shared pool with
   per-agent tool namespace projection? This affects the REST management API shape
   and the runtime lifecycle model.

2. **Plugin capability token authority**: Who signs capability tokens — the gateway
   admin key alone, or does an operator need to explicitly approve each plugin
   capability via a separate approval flow? The former is simpler; the latter gives
   finer-grained control for Tier 3 deployments.

3. **OCI signing policy for air-gapped deployments**: Cosign requires TUF root
   access. For air-gapped or strict-network deployments (Tier 3), what is the
   fallback? Options: local Rekor instance, offline bundle verification, or
   unsigned-but-hash-pinned artifacts.
