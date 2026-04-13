# SPEC: gRPC Plugin Interface (`sera-plugins`)

> **Status:** DRAFT
> **Design decision:** 2026-04-13 — established as a distinct extension point, separate from WASM hooks (SPEC-hooks) and compiled-in backends (SPEC-deployment §1a).
> **Crate:** `sera-gateway` (plugin registry), `sera-plugin-sdk` (client SDK)
> **Priority:** Phase 3

---

## 1. Overview

SERA has **three distinct extension points**. This spec covers the third one — gRPC/RPC plugins. These must not be conflated with the other two:

| Extension point | Mechanism | When to use |
|---|---|---|
| **Compiled-in backends** | All officially supported backends ship in the binary; config selects | Switching between supported implementations (SQLite → Postgres, file memory → Qdrant) |
| **WASM hooks** | Runtime-loaded, sandboxed, synchronous pipeline middleware | Custom authz logic, content policies, context augmentation — small, fast, stateless |
| **gRPC/RPC plugins** | Out-of-process services over gRPC | Custom backends, domain-specific tools, enterprise connectors — stateful, long-lived, any language |

**WASM hooks are not plugins.** WASM hooks are inline middleware — they run synchronously in the event path, are sandboxed by the WASM runtime, and must be stateless and fast (see SPEC-hooks §5). gRPC plugins are external services with their own lifecycle, their own state, and their own process.

**Compiled-in backends are not plugins.** If SERA officially supports PostgreSQL, it is compiled in and selected by config. The plugin interface is for custom or proprietary implementations that are not part of the SERA distribution.

---

## 2. Plugin Contract

A gRPC plugin implements one or more **trait contracts** over the wire. The gateway treats a plugin as a runtime-registered implementation of an internal trait — the same interface it would use for a compiled-in backend.

### 2.1 Plugin Registration

```rust
pub struct PluginRegistration {
    pub name: String,                          // Unique plugin identifier
    pub version: PluginVersion,
    pub capabilities: Vec<PluginCapability>,   // Which trait contracts this plugin implements
    pub endpoint: String,                      // gRPC endpoint (host:port)
    pub tls: Option<TlsConfig>,               // mTLS for production
    pub health_check_interval: Duration,
}

pub enum PluginCapability {
    MemoryBackend,          // Implements the MemoryBackend trait contract
    ToolExecutor,           // Implements the ToolExecutor trait contract
    SandboxProvider,        // Implements the SandboxProvider trait contract
    AuthProvider,           // Implements the AuthProvider trait contract
    SecretProvider,         // Implements the SecretProvider trait contract
    RealtimeBackend,        // Implements the RealtimeBackend trait contract
    Custom(String),         // Domain-specific capability with a registered proto
}
```

### 2.2 Lifecycle

Plugins connect inbound to the gateway at startup or dynamically (hot-registration). The gateway:

1. Validates the plugin's capabilities against its registered proto schemas
2. Performs a health check
3. Registers the plugin in the plugin registry
4. Routes requests to the plugin according to active config

Plugins are long-lived services. They maintain their own state, their own connection pools, and their own error handling. The gateway treats a crashed plugin as a backend failure and applies the same failover logic it would apply to any other backend (retry, circuit break, error propagation).

```protobuf
service PluginRegistry {
    rpc Register(PluginRegistration) returns (RegistrationAck);
    rpc Heartbeat(PluginHeartbeat) returns (HeartbeatAck);
    rpc Deregister(PluginId) returns (Empty);
}
```

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

## 4. Example Plugin: Custom Knowledge Store

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
  endpoint: "knowledge-bridge:9091"

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

The `plugin:` prefix in backend selection routes to the plugin registry rather than the compiled-in backend registry. This is the only config change required — no gateway recompilation.

---

## 5. Plugin SDK

SERA provides a `sera-plugin-sdk` for building plugins in Rust. For other languages, the plugin proto definitions are published and versioned so authors can generate clients with standard tooling.

```
sera/
  proto/
    plugin/
      registry.proto        # Registration and heartbeat
      memory_backend.proto  # MemoryBackend trait contract
      tool_executor.proto   # ToolExecutor trait contract
      sandbox_provider.proto
      auth_provider.proto
      secret_provider.proto
```

Proto files are Apache-2.0. Client stubs are generated by standard `protoc` tooling in the target language.

---

## 6. Security

### mTLS

All plugin connections MUST use mTLS in Tier 2/3 deployments. The gateway validates the plugin's client certificate against a pinned CA. Plain TCP is permitted for localhost-only development.

### Plugin isolation

Plugins run as separate OS processes. They cannot access the gateway's memory, the agent transcripts, or any other gateway-internal state except through the explicit gRPC interface. The gateway never passes raw secrets to plugins — secret resolution happens at the gateway, and only resolved values (or structured references the plugin itself knows how to resolve) are passed.

### Audit

Every plugin invocation is logged with the plugin name, capability, call ID, and duration. Plugin failures are logged as errors with the full error response. Plugins cannot write to the audit log directly.

---

## 7. Invariants

| Invariant | Enforcement |
|---|---|
| Plugins are never in the critical path without explicit config | `backend: plugin:X` must be set; default backends are always compiled-in |
| Plugin crashes do not crash the gateway | Circuit breaker per plugin; gateway applies fallback or returns an error to the agent |
| Plugins cannot impersonate internal components | Registration requires a signed capability token from the gateway admin |
| Plugin invocations are audited | Automatic — audit handle injected into every dispatch call |

---

## 8. Configuration

```yaml
# Plugin registration (auto-loaded from sera.d/)
apiVersion: sera.dev/v1
kind: Plugin
metadata:
  name: my-plugin
spec:
  capabilities: [ToolExecutor]
  endpoint: "localhost:9090"
  tls:
    ca_cert: /etc/sera/plugins/ca.crt
    client_cert: /etc/sera/plugins/client.crt
    client_key: /etc/sera/plugins/client.key
  health_check_interval: 30s

# Activating a plugin as a backend
memory:
  backend: plugin:my-plugin        # Routes to plugin registry

hooks:
  pre_tool:
    - wasm: /hooks/authz.wasm      # WASM hook — NOT a plugin

plugins:
  - name: my-plugin                # Explicit plugin registration (alternative to Kind: Plugin manifest)
    grpc: localhost:9090
```

---

## 9. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-hooks` | [SPEC-hooks](SPEC-hooks.md) | WASM hooks are NOT plugins — distinct extension point; see SPEC-hooks §1a |
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Plugin registry lives in the gateway; plugins cannot bypass gateway AuthZ |
| `sera-tools` | [SPEC-tools](SPEC-tools.md) | ToolExecutor plugins register tools into the tool registry; same dispatch pipeline applies |
| `sera-memory` | [SPEC-memory](SPEC-memory.md) | MemoryBackend plugins implement the MemoryBackend trait contract |
| `sera-deployment` | [SPEC-deployment](SPEC-deployment.md) | Three extension points: compiled-in (this spec is NOT), WASM hooks, gRPC plugins (this spec) |
| `sera-config` | [SPEC-config](SPEC-config.md) | `plugin:X` backend selector syntax; Kind: Plugin manifest |

---

## 10. Open Questions

1. **Plugin versioning** — How are plugin proto contract versions negotiated at registration? Semver? Capability set negotiation?
2. **Plugin discovery** — Should the gateway support auto-discovery of plugins on a local network (mDNS)? Or is explicit config-file registration always required?
3. **Plugin hot-registration** — Can plugins register dynamically without restarting the gateway? Target: yes, but persistence semantics need design.
4. **Capability token signing** — Who signs capability tokens? The operator? The gateway admin key? How are tokens rotated?
5. **Plugin marketplace** — Is there a planned registry/marketplace for community plugins? Out of scope for SERA 1.0.
