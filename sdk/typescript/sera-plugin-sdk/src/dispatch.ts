import { CapabilityNotImplemented } from "./errors.js";
import { ContextEngine, ContextQuery, ContextDiagnostics } from "./capabilities/context_engine.js";
import { MemoryBackend } from "./capabilities/memory_backend.js";

/**
 * Detect which capabilities a plugin object implements by checking for the
 * abstract methods at runtime. The SDK's transport drivers use this to
 * build the registration envelope and to route incoming JSON-RPC methods.
 *
 * Order-sensitive: the keys of the returned map double as the list sent in
 * the registration envelope, and must match the PascalCase names in the
 * Rust `PluginCapability` enum.
 */
export interface CapabilityDetection {
  MemoryBackend: boolean;
  ContextEngine: boolean;
  ContextQuery: boolean;
  ContextDiagnostics: boolean;
}

export function detectCapabilities(plugin: object): CapabilityDetection {
  return {
    MemoryBackend: hasAll(plugin, ["write", "search", "delete"]) &&
      !hasAll(plugin, ["assemble"]), // disambiguate from ContextEngine
    ContextEngine: hasAll(plugin, ["ingest", "assemble"]),
    ContextQuery: hasAll(plugin, ["search", "expand"]),
    ContextDiagnostics: hasAll(plugin, ["status", "doctor"]),
  };
}

function hasAll(obj: object, methods: string[]): boolean {
  return methods.every(
    (m) => typeof (obj as Record<string, unknown>)[m] === "function",
  );
}

export function advertisedCapabilities(plugin: object): string[] {
  const detected = detectCapabilities(plugin);
  return (Object.keys(detected) as Array<keyof CapabilityDetection>).filter(
    (k) => detected[k],
  );
}

/**
 * Dispatch an incoming RPC method to the right plugin handler. Throws
 * `CapabilityNotImplemented` if the plugin doesn't handle the method.
 */
export async function dispatch(
  plugin: object,
  method: string,
  params: unknown,
): Promise<unknown> {
  const fn = (plugin as Record<string, unknown>)[method];
  if (typeof fn !== "function") {
    throw new CapabilityNotImplemented(classifyMethod(method), method);
  }
  return await (fn as (p: unknown) => unknown).call(plugin, params);
}

function classifyMethod(method: string): string {
  if (ContextEngine.prototype && method in ContextEngine.prototype) return "ContextEngine";
  if (ContextQuery.prototype && method in ContextQuery.prototype) return "ContextQuery";
  if (ContextDiagnostics.prototype && method in ContextDiagnostics.prototype) return "ContextDiagnostics";
  if (MemoryBackend.prototype && method in MemoryBackend.prototype) return "MemoryBackend";
  return "Unknown";
}
