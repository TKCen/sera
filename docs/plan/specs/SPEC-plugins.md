# SPEC: Plugin Interface (`sera-plugins`)

> **Status:** DRAFT
> **Amended:** 2026-04-21 (bead sera-pzjk) — dual-transport (stdio + gRPC), `ContextEngine` capability, first-class multi-language SDKs. See §11 for the amendment summary.
> **Design decision:** 2026-04-13 — established as a distinct extension point, separate from WASM hooks (SPEC-hooks) and compiled-in backends (SPEC-deployment §1a).
> **Crate:** `sera-gateway` (plugin registry), `sera-plugin-sdk-{rust,py,ts}` (client SDKs)
> **Priority:** Phase 3

---

## 1. Overview

SERA has **three distinct extension points**. This spec covers the third one — out-of-process plugins. These must not be conflated with the other two:

| Extension point | Mechanism | When to use |
|---|---|---|
| **Compiled-in backends** | All officially supported backends ship in the binary; config selects | Switching between supported implementations (SQLite → Postgres, file memory → Qdrant) |
| **WASM hooks** | Runtime-loaded, sandboxed, synchronous pipeline middleware | Custom authz logic, content policies, context augmentation — small, fast, stateless |
| **Out-of-process plugins** | Out-of-process services over **gRPC or stdio** | Custom backends, domain-specific tools, enterprise connectors, language-agnostic engines — stateful, long-lived, any language |

**WASM hooks are not plugins.** WASM hooks are inline middleware — they run synchronously in the event path, are sandboxed by the WASM runtime, and must be stateless and fast (see SPEC-hooks §5). Third-party WASM/WIT hook surface work (bead `sera-s4b1`) is orthogonal to this spec and proceeds independently — the three-extension-points framing is unchanged by this amendment.

**Compiled-in backends are not plugins.** If SERA officially supports PostgreSQL, it is compiled in and selected by config. The plugin interface is for custom or proprietary implementations that are not part of the SERA distribution.

**Two transports, one contract.** A plugin may be reached over **gRPC** (TCP, mTLS, remote or localhost) or **stdio** (spawned child process, stdin/stdout framed JSON-RPC). Both transports expose the same `PluginCapability` enum, the same `Kind: Plugin` manifest shape, and the same supervision and audit semantics. Transport selection is an **operations/deployment concern, not a code/spec concern** — sera does not gate transport choice on tier, environment, or "dev vs prod." Operators pick whichever transport fits their ops posture (image distribution, network topology, runtime sandboxing, language toolchain); the gateway treats them uniformly. See §2.3.

---

## 2. Plugin Contract

A plugin implements one or more **trait contracts** over the wire (either gRPC or stdio — see §2.3). The gateway treats a plugin as a runtime-registered implementation of an internal trait — the same interface it would use for a compiled-in backend.

### 2.1 Plugin Registration

```rust
pub struct PluginRegistration {
    pub name: String,                          // Unique plugin identifier
    pub version: PluginVersion,
    pub capabilities: Vec<PluginCapability>,   // Which trait contracts this plugin implements
    pub transport: PluginTransport,            // stdio | gRPC (see §2.3)
    pub health_check_interval: Duration,
}

pub enum PluginCapability {
    MemoryBackend,          // Implements the MemoryBackend trait contract
    ToolExecutor,           // Implements the ToolExecutor trait contract
    ContextEngine,          // Implements the ContextEngine / ContextQuery / ContextDiagnostics triad (SPEC-context-engine-pluggability)
    SandboxProvider,        // Implements the SandboxProvider trait contract
    AuthProvider,           // Implements the AuthProvider trait contract
    SecretProvider,         // Implements the SecretProvider trait contract
    RealtimeBackend,        // Implements the RealtimeBackend trait contract
    Custom(String),         // Domain-specific capability with a registered proto / schema
}
```

> **Amendment note (2026-04-21):** The `ContextEngine` variant is **new**. It opens the context-engine seam established in [SPEC-context-engine-pluggability](SPEC-context-engine-pluggability.md) to out-of-process implementations. That spec's §8 explicitly defers extracting a dedicated `sera-context-engine` crate until a *second* `ContextEngine` implementation justifies the move; an out-of-process LCM plugin (see §4b) is the first such second implementation, which means this amendment is what lets sera-context-engine-pluggability §8's crate-extraction gate trip. See §4b for the LCM worked example.
>
> The current `rust/crates/sera-plugins` code ships the `MemoryBackend / ToolExecutor / SandboxProvider / AuthProvider / SecretProvider / RealtimeBackend / Custom` variants without `ContextEngine`. Adding the variant to `PluginCapability` in `rust/crates/sera-plugins/src/types.rs` and wiring the `transport:` field into `ManifestSpec` in `manifest.rs` are tracked as **follow-up implementation beads**, not part of this markdown amendment. The amendment captures the accepted design; the Rust surface catches up in those follow-ups.

### 2.2 Lifecycle

Plugins are registered at startup via `Kind: Plugin` manifests (§8), or dynamically via hot-registration. The gateway:

1. Validates the plugin's capabilities against its registered schemas (proto for gRPC, JSON Schema mirror for stdio — see §5)
2. Performs a health check (heartbeat over the selected transport)
3. Registers the plugin in the plugin registry
4. Routes requests to the plugin according to active config

Plugins are long-lived services. They maintain their own state, their own connection pools, and their own error handling. The gateway treats a crashed plugin as a backend failure and applies the same failover logic it would apply to any other backend (retry, circuit break, error propagation). Supervision is transport-uniform — see §6 Supervision.

```protobuf
// Shared PluginRegistry service — identical surface for stdio and gRPC.
// The proto is canonical; the stdio JSON-RPC mirror follows the same method set.
service PluginRegistry {
    rpc Register(PluginRegistration) returns (RegistrationAck);
    rpc Heartbeat(PluginHeartbeat) returns (HeartbeatAck);
    rpc Deregister(PluginId) returns (Empty);
}
```

### 2.3 Transport

Plugins advertise exactly one transport per registration. The two transports differ only in how bytes flow between the gateway and the plugin process; the **capability set, the wire contract, the manifest shape, the supervision model, and the audit envelope are identical**.

```rust
pub enum PluginTransport {
    /// gRPC over TCP. Remote or localhost; mTLS required in Tier 2/3.
    Grpc {
        endpoint: String,           // host:port
        tls: Option<TlsConfig>,     // required outside localhost dev
    },
    /// Child process spawned by the gateway. stdin/stdout carry
    /// framed JSON-RPC matching the proto-defined method set.
    Stdio {
        command: Vec<String>,       // argv for the plugin process
        env: HashMap<String, String>,
        // Optional Unix-domain socket for heartbeat / out-of-band control.
        // If unset, heartbeat multiplexes over the main stdio channel.
        control_socket: Option<PathBuf>,
    },
}
```

**Protocol alignment with SPEC-hooks §2.6.** The stdio transport is the **same subprocess pattern** SERA already uses for subprocess hooks in [SPEC-hooks](SPEC-hooks.md) §2.6, itself sourced from [SPEC-dependencies](SPEC-dependencies.md) §10.1 (claw-code subprocess pattern): the gateway writes a framed JSON request to the plugin's stdin, the plugin writes a framed JSON response to its stdout, and `stderr` is reserved for human-readable logs. The difference vs §2.6 is scope — a subprocess *hook* is invoked per event and exits (or is pooled for a short window); a subprocess *plugin* is a long-lived child that stays up for the lifetime of its registration. The envelope format is the same JSON-RPC surface derived from `rust/proto/plugin/*.proto`, so a reviewer who already understands §2.6 understands the stdio plugin wire.

**Why both are first-class.** A plugin that needs to run in a separate failure domain, on a different host, or behind a network boundary uses gRPC. A plugin that wants minimal ops — no TCP port, no cert rotation, process co-located with the gateway, restart controlled by the gateway — uses stdio. **Neither is a "dev" mode and neither is a "prod" mode.** The choice is an operations posture, not a code-path gate.

**Capability parity.** Every `PluginCapability` variant in §2.1 can be served by either transport. There is no "MemoryBackend is gRPC-only" or "ContextEngine is stdio-only" carve-out. The LCM `ContextEngine` example in §4b uses stdio because its Python implementation is simplest over stdin/stdout; the SharePoint `MemoryBackend` example in §4a uses gRPC because it fronts a remote enterprise service — but either could legally run on the other transport, and the gateway's dispatch code path is the same.

---

## 3. Example Plugin: Siemens S7 PLC Connector

This is the canonical motivating example. A Siemens S7 PLC connector is:

- Written in any language (Python, Go, C++ via the Siemens SDK)
- Maintains long-lived TCP connections to PLC endpoints
- Exposes PLC data as SERA tools (`plc_read_tag`, `plc_write_tag`, `plc_read_block`, `plc_alarm_list`)
- Owns its own connection lifecycle — reconnect, heartbeat, alarm subscription
- Registers with the gateway as a `ToolExecutor` plugin

The gateway treats `plc_*` tools exactly like any other tool — same `pre_tool` hook chains, same AuthZ checks, same audit log. The PLC connector sees only resolved, authorized tool dispatch requests; it never talks directly to agents.

```yaml
# sera.d/plugins/s7-connector.yaml
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: s7-plc-connector
spec:
  capabilities:
    - ToolExecutor
  endpoint: "localhost:9090"
  tls:
    ca_cert: /etc/sera/plugins/s7/ca.crt
    client_cert: /etc/sera/plugins/s7/client.crt
    client_key: /etc/sera/plugins/s7/client.key
  health_check_interval: 30s
  tools:
    - name: plc_read_tag
      description: "Read a tag value from a connected S7 PLC"
      risk_level: Read
    - name: plc_write_tag
      description: "Write a value to a tag in a connected S7 PLC"
      risk_level: Write
```

---

## 4a. Example Plugin: Custom Knowledge Store (gRPC MemoryBackend)

An enterprise with a proprietary knowledge base (SharePoint, Confluence, internal wiki) can implement a `MemoryBackend` plugin:

```yaml
# sera.d/plugins/sharepoint-memory.yaml
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: sharepoint-knowledge
spec:
  capabilities:
    - MemoryBackend
  transport: grpc
  grpc:
    endpoint: "knowledge-bridge:9091"
    tls:
      ca_cert: /etc/sera/plugins/sharepoint/ca.crt
      client_cert: /etc/sera/plugins/sharepoint/client.crt
      client_key: /etc/sera/plugins/sharepoint/client.key
  health_check_interval: 30s

# sera.d/instance.yaml — activate the plugin as the memory backend
---
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: my-sera
spec:
  memory:
    backend: plugin:sharepoint-knowledge    # selects the registered plugin
```

The `plugin:` prefix in backend selection routes to the plugin registry rather than the compiled-in backend registry. This is the only config change required — no gateway recompilation. gRPC is the natural fit here because the plugin fronts a remote enterprise service reached over the network; mTLS is mandatory (§6 Security).

---

## 4b. Example Plugin: LCM Context Engine (stdio ContextEngine)

The [OpenClaw](../../docs/plan/specs/SPEC-context-engine-pluggability.md) LCM engine (`hermes-agent/plugins/context_engine/lcm`, ~250 KB of Python with `engine.py` at ~53 KB) is the canonical motivating example for the new `ContextEngine` capability. LCM implements the full trait triad from [SPEC-context-engine-pluggability](SPEC-context-engine-pluggability.md) §2 — `ContextEngine` (core), `ContextQuery` (drill tools), `ContextDiagnostics` (status/doctor) — and its agent-facing drill tools (`lcm_grep`, `lcm_describe`, `lcm_expand`, `lcm_expand_query`, `lcm_status`, `lcm_doctor`) are the public surface the LLM reaches back into compacted history with.

Wrapping LCM as a stdio plugin is the shortest path to exercising `ContextEngine` as a plugin seam without a Rust port — the Python SDK (§5) provides the stdin/stdout JSON-RPC loop and the capability ABCs, and the plugin author subclasses and maps the trait methods onto LCM's existing Python API. The mapping is the one already specified in SPEC-context-engine-pluggability §4 (LCM worked example); this plugin wires that mapping to the gateway over stdio.

```yaml
# sera.d/plugins/lcm-context.yaml
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: lcm-context
spec:
  capabilities:
    - ContextEngine
  transport: stdio
  stdio:
    command: ["python", "-m", "sera_context_lcm"]
    env:
      LCM_DB_PATH: "/var/lib/sera/lcm.db"
      PYTHONUNBUFFERED: "1"
  health_check_interval: 30s

# sera.d/instance.yaml — activate the plugin as the context engine
---
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: my-sera
spec:
  context_engine:
    backend: plugin:lcm-context
```

This is how [SPEC-context-engine-pluggability](SPEC-context-engine-pluggability.md) §8's "second implementation justifies trait-crate extraction" gate trips — once `plugin:lcm-context` is routable as a `ContextEngine`, the seam has proven that the default `ContextPipeline` is not the only conforming implementation, and the follow-up extraction of `sera-context-engine` to a dedicated crate becomes ripe. That crate extraction is a sibling bead; the amendment here is the contract that unlocks it.

Stdio is the natural fit because LCM's implementation is a local Python process with a SQLite database on disk — there is no remote service to reach, no multi-host concern, and spawning a child under the gateway's supervision tree avoids the operator having to run a separate daemon with its own port and TLS material. A future operator who wants to run LCM on a different host can flip `transport:` to `grpc` (wrapping the same Python code with `grpcio`) without changing either the capability declaration or any consumer wiring.

---

## 5. Plugin SDK

SERA ships **three first-class plugin SDKs**, one per language, all supporting **both transports** (gRPC and stdio). The motivation is ecosystem-opening and floor-lowering — a contributor (human or agent) asked to write a sera plugin should reach for their language of choice and find an SDK that hides the transport mechanics behind capability ABCs / traits / decorators, not a bag of protoc artifacts and a transport README.

| SDK | Crate / package | Languages | Transports |
|---|---|---|---|
| `sera-plugin-sdk-rust` | `rust/crates/sera-plugin-sdk` | Rust | stdio + gRPC |
| `sera-plugin-sdk-py` | `sdks/py/sera-plugin-sdk` (PyPI: `sera-plugin-sdk`) | Python 3.11+ | stdio + gRPC |
| `sera-plugin-sdk-ts` | `sdks/ts/sera-plugin-sdk` (npm: `@sera/plugin-sdk`) | TypeScript / Node 20+ | stdio + gRPC |

The Python and TypeScript SDKs are **not** "generate protoc and figure it out yourself." They are thin ergonomic wrappers — classes / ABCs with `@capability` or `@tool` decorators, a runtime that handles framing, heartbeats, graceful shutdown, and error-envelope conversion — so a plugin author writes plugin logic, not transport plumbing. A Python `ContextEngine` plugin looks roughly like:

```python
# sera_context_lcm/__main__.py — schematic
from sera_plugin_sdk import run_stdio_plugin, ContextEngine, ContextQuery, ContextDiagnostics

class LcmPlugin(ContextEngine, ContextQuery, ContextDiagnostics):
    async def ingest(self, msg): ...
    async def assemble(self, budget): ...
    async def search(self, req): ...
    async def status(self, session_id): ...

if __name__ == "__main__":
    run_stdio_plugin(LcmPlugin())
```

### 5.1 Schema canonicalisation

Proto files remain the **canonical** wire contract. Each `.proto` in `rust/proto/plugin/` gets a **hand-maintained `.schema.json` sibling** that describes the same wire format for stdio JSON-RPC consumers. The JSON Schemas are **not** codegen output — they are authored and reviewed alongside the protos, committed next to them, and checked at PR time. If proto and JSON Schema drift, **proto wins**; reviewers are responsible for catching the drift before merge. This keeps the two-transport story honest without introducing a build-step dependency on a proto-to-JSON-Schema generator.

```
rust/proto/plugin/
  registry.proto          registry.schema.json         # Registration + heartbeat
  memory_backend.proto    memory_backend.schema.json   # MemoryBackend trait
  tool_executor.proto     tool_executor.schema.json    # ToolExecutor trait
  context_engine.proto    context_engine.schema.json   # ContextEngine + ContextQuery + ContextDiagnostics (new)
  sandbox_provider.proto  sandbox_provider.schema.json
  auth_provider.proto     auth_provider.schema.json
  secret_provider.proto   secret_provider.schema.json
```

Proto and schema files are Apache-2.0. A plugin author who needs raw wire access (no SDK) can consume either the proto (via `protoc`) or the schema (via any JSON Schema tool) and build against the contract directly — the SDKs are ergonomics, not a gate.

### 5.2 SDK follow-up beads

The three SDK crates, the `sera-context-lcm` Python plugin, and the manifest `transport:` field wiring are tracked as sibling beads under parent `sera-xx48`. This spec is the design record they implement against; none of those implementations ship as part of this amendment.

---

## 6. Security

Security requirements are **transport-uniform in the spec, operator-satisfied differently**. The code treats "this plugin is authenticated and authorised" the same way regardless of transport; the operator satisfies that contract with mTLS certs for gRPC and with filesystem/socket permissions for stdio. Neither transport is "more secure" than the other — each has its own attack surface and its own mitigation.

### 6.1 mTLS (gRPC transport)

All **gRPC** plugin connections MUST use mTLS in Tier 2/3 deployments. The gateway validates the plugin's client certificate against a pinned CA. Plain TCP is permitted for localhost-only development.

### 6.2 Socket permissions (stdio transport)

All **stdio** plugin connections MUST run with sufficiently restricted filesystem and socket permissions that only the gateway's UID can reach the plugin's IO channels. Operators satisfy this by:

- Running the gateway and spawned plugin processes under the same UID.
- If the stdio transport uses an auxiliary Unix-domain socket (`control_socket` in the `PluginTransport::Stdio` variant — e.g. for out-of-band heartbeat or capability-token exchange), the socket file MUST be created with mode `0600` (owner-only) in a directory that is itself owner-only (`0700`).
- On Windows, named-pipe parity: the equivalent is a named pipe with a DACL that grants access only to the gateway's user SID. See §10 Open Questions on Windows parity.

In all cases the gateway refuses to register a stdio plugin whose control socket or pipe has looser permissions. This is the socket-perms analog of mTLS cert pinning — the gateway rejects any transport where it cannot prove the peer is who the operator declared.

### 6.3 Plugin isolation

Plugins run as separate OS processes regardless of transport. They cannot access the gateway's memory, the agent transcripts, or any other gateway-internal state except through the explicit plugin wire contract (gRPC or stdio JSON-RPC). The gateway never passes raw secrets to plugins — secret resolution happens at the gateway, and only resolved values (or structured references the plugin itself knows how to resolve) are passed.

### 6.4 Audit

Every plugin invocation is logged with the plugin name, capability, call ID, transport, and duration. Plugin failures are logged as errors with the full error response. Plugins cannot write to the audit log directly. The audit envelope is identical across transports — the only extra field is `transport: "grpc" | "stdio"` so operators can correlate plugin behaviour to transport choice.

### 6.5 Supervision

Supervision is uniform across transports. The `CircuitBreaker` (`rust/crates/sera-plugins/src/circuit_breaker.rs`) and the `health_check_interval` heartbeat loop already cover the gRPC path; the stdio path reuses both, and adds subprocess lifecycle management:

| Supervision concern | gRPC | stdio |
|---|---|---|
| Health check | `Heartbeat` RPC every `health_check_interval` | Same `Heartbeat` JSON-RPC method over stdin/stdout (or the optional `control_socket`) |
| Failure isolation | Per-plugin `CircuitBreaker` (3-state: closed → open → half-open) | Same `CircuitBreaker` — same failure counts, same cooldown |
| Crash detection | Connection drop / RPC timeout | Child-process exit (non-zero status or SIGPIPE on write) |
| Restart | Operator or supervisor reconnects; gateway records failures via breaker | Gateway respawns the child with exponential backoff; breaker tracks consecutive restart failures |
| Graceful shutdown | Deregister RPC, then close channel | `SIGTERM` on the child, wait `shutdown_grace` (default 5s), then `SIGKILL` |

The subprocess lifecycle above — spawn on register, `SIGTERM` on shutdown, `SIGKILL` after grace, exponential backoff on restart — is the stdio-specific bookkeeping on top of the shared `CircuitBreaker` + heartbeat model. It is **not** a second supervision framework; it is the same breaker with an extra lifecycle hook for the process. The heartbeat semantic is the same across both transports: N consecutive failures trip the breaker, the breaker gates dispatch, a half-open probe succeeds to close.

---

## 7. Invariants

| Invariant | Enforcement |
|---|---|
| Plugins are never in the critical path without explicit config | `backend: plugin:X` must be set; default backends are always compiled-in |
| Plugin crashes do not crash the gateway | Circuit breaker per plugin; gateway applies fallback or returns an error to the agent |
| Plugins cannot impersonate internal components | Registration requires a signed capability token from the gateway admin (transport-uniform — verified over the same auth envelope on gRPC and stdio) |
| Plugin invocations are audited | Automatic — audit handle injected into every dispatch call, transport recorded in the envelope |
| Transport choice is not a security carve-out | Both transports satisfy §6.1 / §6.2 / §6.3 / §6.4 / §6.5 before the gateway will dispatch to them |

---

## 8. Configuration

Two transport variants, same `Kind: Plugin` manifest. Operators author one of these per plugin:

```yaml
# sera.d/plugins/my-grpc-plugin.yaml — gRPC transport
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
```

```yaml
# sera.d/plugins/my-stdio-plugin.yaml — stdio transport
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: my-stdio-plugin
spec:
  capabilities: [ContextEngine]
  transport: stdio
  stdio:
    command: ["python", "-m", "my_plugin"]
    env:
      MY_PLUGIN_CONFIG: "/etc/sera/plugins/my-plugin.toml"
    # Optional — for out-of-band heartbeat / capability-token exchange.
    # Must be 0600 in a 0700 directory; see §6.2.
    control_socket: "/run/sera/plugins/my-stdio-plugin.sock"
  health_check_interval: 30s
```

```yaml
# Activating a plugin as a backend (transport-agnostic)
memory:
  backend: plugin:my-grpc-plugin        # Routes to plugin registry
context_engine:
  backend: plugin:my-stdio-plugin

hooks:
  pre_tool:
    - wasm: /hooks/authz.wasm           # WASM hook — NOT a plugin

plugins:
  - name: my-grpc-plugin                # Explicit plugin registration (alternative to Kind: Plugin manifest)
    transport: grpc
    grpc: { endpoint: localhost:9090 }
  - name: my-stdio-plugin
    transport: stdio
    stdio:
      command: ["python", "-m", "my_plugin"]
```

> **Amendment note (2026-04-21):** The current Rust manifest parser (`rust/crates/sera-plugins/src/manifest.rs::ManifestSpec`) does **not** yet know about a `transport:` field — it reads `endpoint:` and optional `tls:` at the top of `spec:` and is effectively gRPC-only today. Teaching the parser about `transport: grpc | stdio` + the two sub-blocks is a **follow-up implementation bead**, not part of this markdown amendment. The amendment here is the accepted target shape; the parser catches up next.

---

## 9. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) §1a, §2.6 | WASM hooks are NOT plugins — distinct extension point. **Stdio plugin transport shares the subprocess pattern and supervision semantics with subprocess hooks (§2.6)**; both are the same protocol family sourced from SPEC-dependencies §10.1. |
| `sera-memory-pluggability` | [SPEC-memory-pluggability](SPEC-memory-pluggability.md) | **Sibling pluggability pattern** — same "trait is honest about what it models, the things it doesn't model get their own trait" framing. A `MemoryBackend` plugin implements the `SemanticMemoryStore` contract defined there. |
| `sera-context-engine-pluggability` | [SPEC-context-engine-pluggability](SPEC-context-engine-pluggability.md) §8 | **First `ContextEngine` plugin consumer**. §8 defers extracting `sera-context-engine` to a dedicated crate until a second impl justifies it; the `plugin:lcm-context` example in §4b is that second impl. Amendment here + LCM plugin = trait-crate extraction gate trips. |
| `sera-dependencies` | [SPEC-dependencies](SPEC-dependencies.md) §10.1 | **Protocol source.** The stdio JSON-RPC pattern used for plugins is the same claw-code subprocess pattern SPEC-hooks §2.6 references — stdin JSON in / stdout JSON out, `HookRunResult`-style typed responses. |
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Plugin registry lives in the gateway; plugins cannot bypass gateway AuthZ on either transport |
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | ToolExecutor plugins register tools into the tool registry; same dispatch pipeline applies regardless of transport |
| `sera-memory` | [SPEC-memory](SPEC-memory.md) | MemoryBackend plugins implement the MemoryBackend trait contract |
| `sera-deployment` | [SPEC-deployment](SPEC-deployment.md) | Three extension points: compiled-in (NOT this spec), WASM hooks, out-of-process plugins (this spec — gRPC + stdio) |
| `sera-config` | [SPEC-config](SPEC-config.md) | `plugin:X` backend selector syntax; `Kind: Plugin` manifest (unchanged shape; new `transport:` field per §2.3 and §8) |

---

## 10. Open Questions

### 10.1 Pre-existing (unchanged by this amendment)

1. **Plugin versioning** — How are plugin proto / JSON Schema contract versions negotiated at registration? Semver? Capability set negotiation?
2. **Plugin discovery** — Should the gateway support auto-discovery of plugins on a local network (mDNS)? Or is explicit config-file registration always required?
3. **Plugin hot-registration** — Can plugins register dynamically without restarting the gateway? Target: yes, but persistence semantics need design.
4. **Capability token signing** — Who signs capability tokens? The operator? The gateway admin key? How are tokens rotated?
5. **Plugin marketplace** — Is there a planned registry/marketplace for community plugins? Out of scope for SERA 1.0.

### 10.2 Opened by the 2026-04-21 amendment

6. **Stdio socket path convention across OSes.** Where should the gateway create stdio `control_socket` files by default? `/run/sera/plugins/*.sock` is the Linux convention; macOS differs; Windows has no socket. Per-OS defaults + an explicit `control_socket:` override seems right, but the defaults need to be pinned before SDK work lands.
7. **Windows named-pipe parity.** On Windows, the stdio transport's `control_socket` equivalent is a named pipe with a DACL granting access only to the gateway's user SID. Does the Rust manifest model a single `control_socket: PathBuf` field (interpreting it as a pipe name on Windows) or a discriminated `control_channel: { Unix { path } | Pipe { name } }`? Leaning toward the former for simplicity, but the DACL construction is non-trivial.
8. **SDK versioning across three languages.** Rust, Python, and TypeScript SDKs all consume the same proto + JSON Schema, but they ship on independent cadences (crates.io, PyPI, npm). Does sera publish a **coordinated release** (all three at the same contract version) or **independent releases** gated by the proto version they target? Coordinated is simpler to reason about; independent is the reality of multi-ecosystem publishing.
9. **JSON Schema drift detection.** The amendment specifies that protos are canonical and JSON Schemas are hand-maintained mirrors, with reviewers catching drift at PR time. Is that sufficient, or does the repo need a CI check that diff-compares the generated proto descriptor against the JSON Schema? The codegen-free stance says "reviewer catch"; the safety-net stance says "CI check." Leaving the decision open for the SDK beads.
10. **Per-capability transport preference.** Even though capability parity (§2.3) is required, should the spec (or the capability proto) advertise a **recommended** transport per capability? For example, `ContextEngine` is typically local-to-the-gateway, so stdio is recommended; `ToolExecutor` for an external service is typically gRPC. Recommendation vs requirement is the question.

---

## 11. Amendment log

**2026-04-21 (bead `sera-pzjk`, parent `sera-xx48`).** Extension-model amendment resulting from the 2026-04-21 design pass:

1. **Dual transport.** Plugins may register over either `grpc` or `stdio`. Both are first-class; transport choice is an ops/deployment concern, not a code/spec gate. No dev-vs-prod split, no tier-aware default. (§1 Overview, §2.3 Transport, §8 Configuration.)
2. **Stdio aligned with SPEC-hooks §2.6.** The stdio wire is the same subprocess JSON-RPC pattern already used for subprocess hooks, sourced from SPEC-dependencies §10.1. Cross-referenced explicitly rather than reinvented. (§2.3, §9.)
3. **`ContextEngine` capability.** Added to `PluginCapability` enum. First consumer is the Python LCM plugin (§4b); satisfies the trait-crate extraction gate in SPEC-context-engine-pluggability §8. (§2.1, §4b.)
4. **Three first-class SDKs.** `sera-plugin-sdk-{rust,py,ts}`, all supporting both transports, ergonomic wrappers rather than "generate protoc yourself." (§5.)
5. **Canonical proto + hand-maintained JSON Schema mirrors.** No codegen pipeline; reviewers enforce drift. (§5.1.)
6. **Uniform security, operator-satisfied differently.** mTLS for gRPC, socket permissions for stdio — same spec-level requirement, different satisfaction. (§6.1, §6.2.)
7. **Uniform supervision.** `CircuitBreaker` + heartbeat model extends to stdio subprocess lifecycle; not a second supervision framework. (§6.5.)
8. **Manifest `transport:` field.** `Kind: Plugin` gains `transport:` + `grpc:` / `stdio:` sub-blocks. The Rust manifest parser wiring is a follow-up implementation bead. (§8.)
9. **Renamed §4 → §4a + §4b.** Existing SharePoint `MemoryBackend` example is now §4a (gRPC); new LCM `ContextEngine` example is §4b (stdio). Both capabilities can run on either transport — the examples pick the natural fit.
10. **§1 three-extension-points framing preserved.** WASM hook surface (`sera-s4b1`) is orthogonal; one-line cross-reference added in §1.
11. **Preserved verbatim:** §3 S7 PLC example, the core structure of §7 Invariants (one new row added for transport uniformity), existing `PluginCapability` variants.
12. **Spec title and top matter updated** — "gRPC Plugin Interface" → "Plugin Interface"; SDK line expanded to reference all three language SDKs.

**Follow-up implementation beads** tracked under parent `sera-xx48`:

- `sera-plugin-sdk-py` — Python SDK (stdio + gRPC, `MemoryBackend` + `ContextEngine` + `ToolExecutor`)
- `sera-plugin-sdk-ts` — TypeScript SDK (parity with Python SDK)
- `sera-plugin-sdk-rust` — extension of existing `sera-plugins` surface to add stdio transport + `ContextEngine` capability variant + `transport:` manifest field parsing
- `sera-context-lcm` — first Python plugin consumer, wraps `hermes-agent/plugins/context_engine/lcm`
- A bead for the `rust/crates/sera-plugins` code changes itself (add `ContextEngine` to `PluginCapability`, extend `ManifestSpec` with `transport:` discriminated block, wire subprocess lifecycle supervision into the registry)
