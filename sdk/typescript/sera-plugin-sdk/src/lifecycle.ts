/**
 * Optional startup/shutdown hooks. A plugin may declare either, both, or
 * neither. The transport driver calls `onStartup` once before dispatching
 * any capability methods, and `onShutdown` once after the transport closes
 * (stdio: stdin EOF; gRPC: server.tryShutdown).
 *
 * Naming: spec and Python SDK use `on_startup` / `on_shutdown`; the TS SDK
 * uses camelCase per language convention. The lifecycle semantics are
 * identical across both SDKs.
 */
export interface Lifecycle {
  onStartup?(): Promise<void>;
  onShutdown?(): Promise<void>;
}

export function hasLifecycle(obj: unknown): obj is Lifecycle {
  return typeof obj === "object" && obj !== null;
}
