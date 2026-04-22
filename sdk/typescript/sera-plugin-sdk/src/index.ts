/**
 * `@sera/plugin-sdk` — TypeScript SDK for authoring SERA plugins.
 *
 * See `SPEC-plugins` §5. Match the Python SDK shape: a plugin is a class
 * that implements one or more capability interfaces, with optional
 * `onStartup` / `onShutdown` lifecycle hooks, dispatched over stdio or
 * gRPC transport.
 *
 * ```ts
 * import {
 *   ContextEngine, ContextQuery, ContextDiagnostics,
 *   runStdioPlugin,
 * } from "@sera/plugin-sdk";
 *
 * class LcmPlugin implements ContextEngine, ContextQuery, ContextDiagnostics {
 *   async ingest(msg) { ... }
 *   async assemble(budget) { ... }
 *   async search(req) { ... }
 *   async expand(id) { ... }
 *   async status(sessionId) { ... }
 *   async doctor() { ... }
 *   async onStartup() { ... }
 *   async onShutdown() { ... }
 * }
 *
 * await runStdioPlugin(new LcmPlugin(), { name: "lcm-context" });
 * ```
 */

export * from "./errors.js";
export * from "./capabilities/index.js";
export * from "./transport/index.js";
export type { Lifecycle } from "./lifecycle.js";
export {
  PROTOCOL_VERSION,
  PluginCapabilityName,
  type PluginVersion,
  type PluginTransport,
  type GrpcTransportConfig,
  type StdioTransportConfig,
  type TlsConfig,
  type PluginRegistration,
} from "./types.js";
export { advertisedCapabilities, detectCapabilities } from "./dispatch.js";
