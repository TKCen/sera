# @sera/plugin-sdk

TypeScript SDK for authoring SERA plugins. Dual transport (stdio + gRPC), one
class per capability, async/await everywhere. See
[SPEC-plugins](../../../docs/plan/specs/SPEC-plugins.md) §5.

## Install

```bash
npm install @sera/plugin-sdk
```

Node 20+, ESM only.

## Quick start

```ts
import {
  ContextEngine,
  ContextQuery,
  ContextDiagnostics,
  runStdioPlugin,
} from "@sera/plugin-sdk";

class LcmPlugin implements ContextEngine, ContextQuery, ContextDiagnostics {
  async ingest(msg) { /* ... */ }
  async assemble(budget) { /* ... */ }
  async search(req) { /* ... */ }
  async expand(id) { /* ... */ }
  async status(sessionId) { /* ... */ }
  async doctor() { /* ... */ }

  async onStartup() { /* open DB, warm caches */ }
  async onShutdown() { /* flush, close */ }
}

await runStdioPlugin(new LcmPlugin(), { name: "lcm-context" });
```

## Capabilities

Capability contracts are declared as abstract classes. A plugin `implements`
any combination — TypeScript checks that every abstract method is provided.

| Capability           | Import                 | Methods                                    |
| -------------------- | ---------------------- | ------------------------------------------ |
| `ContextEngine`      | `@sera/plugin-sdk`     | `ingest`, `assemble`                       |
| `ContextQuery`       | `@sera/plugin-sdk`     | `search`, `expand`                         |
| `ContextDiagnostics` | `@sera/plugin-sdk`     | `status`, `doctor`                         |
| `MemoryBackend`      | `@sera/plugin-sdk`     | `write`, `search`, `delete`                |

## Transports

- **stdio** — `runStdioPlugin(plugin, opts)` spawns a newline-delimited
  JSON-RPC 2.0 loop over `stdin`/`stdout`. Heartbeats multiplex on the same
  channel (SPEC-plugins §2.3, Q7 resolution).
- **gRPC** — `runGrpcPlugin(plugin, opts)` binds a `@grpc/grpc-js` server.
  Requires `rust/proto/plugin/` to be reachable at build time (pre-generated
  stubs) or at runtime via `options.protoDir` / `SERA_PROTO_DIR`.

## Lifecycle

Optional hooks, both `async`:

- `onStartup()` — awaited before the transport loop starts.
- `onShutdown()` — awaited on `SIGTERM` / `SIGINT` / stdin EOF.

## Errors

`PluginError` is the base class. Subclasses mirror the Rust
`PluginError` enum in `rust/crates/sera-plugins/src/error.rs` —
`RegistrationFailed`, `HealthCheckFailed`, `PluginNotFound`,
`PluginUnhealthy`, `ManifestInvalid`, `CircuitOpen`, `Unauthorized`,
`ConnectionFailed`, plus SDK-only `CapabilityNotImplemented` and
`ProtocolError`.

Throw any `PluginError` from a capability method — the transport wraps
it into a JSON-RPC error response and does not crash the plugin process.

## Build

```bash
npm install        # deps
npm run build      # runs protoc (if protos present) then tsup
npm test
npm run typecheck
```

Proto generation is gated by the presence of `rust/proto/plugin/`. When the
proto directory is absent, the SDK still builds — stdio and authored
capability types do not depend on generated gRPC stubs.

## Release cadence

Independent, per SPEC-plugins §5.3. Each SDK pins to a `PROTOCOL_VERSION`
(see `src/types.ts`); the protocol version bump is the coordination point
across all three language SDKs.
