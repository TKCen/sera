import { createInterface } from "node:readline";
import { once } from "node:events";
import { advertisedCapabilities, dispatch } from "../dispatch.js";
import { PluginError, ProtocolError } from "../errors.js";
import { type Lifecycle } from "../lifecycle.js";
import { PROTOCOL_VERSION } from "../types.js";

/**
 * JSON-RPC 2.0 request envelope read from stdin.
 * Framing: one JSON object per line (newline-delimited JSON), matching
 * SPEC-hooks §2.6 / SPEC-dependencies §10.1 claw-code subprocess pattern.
 */
interface JsonRpcRequest {
  jsonrpc: "2.0";
  id: number | string | null;
  method: string;
  params?: unknown;
}

interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: number | string | null;
  result?: unknown;
  error?: { code: number; message: string; data?: unknown };
}

export interface RunStdioPluginOptions {
  /** Plugin name sent in the Register reply. Defaults to `process.env.SERA_PLUGIN_NAME ?? "unknown"`. */
  name?: string;
  /** Plugin semver. Defaults to `0.0.1`. */
  version?: { major: number; minor: number; patch: number };
  /** Streams to use — overridable for testing. */
  input?: NodeJS.ReadableStream;
  output?: NodeJS.WritableStream;
  /** Signals to trap for graceful shutdown. Default: SIGTERM, SIGINT. */
  signals?: NodeJS.Signals[];
}

const BUILTIN_METHODS = new Set(["Register", "Heartbeat", "Deregister"]);

/**
 * Run a plugin over stdio. Reads newline-delimited JSON-RPC 2.0 requests
 * from stdin, dispatches `method` to the plugin instance, and writes
 * responses to stdout. Heartbeats multiplex over the same channel
 * (SPEC-plugins §2.3, Q7 resolution).
 *
 * Lifecycle:
 *   1. `plugin.onStartup?.()` — awaited before the read loop begins.
 *   2. Read loop dispatches every incoming JSON-RPC request.
 *   3. On SIGTERM / SIGINT / stdin EOF: `plugin.onShutdown?.()` is awaited, then the process exits 0.
 *
 * Errors thrown by capability methods are converted to JSON-RPC error
 * responses — they do not crash the plugin process.
 */
export async function runStdioPlugin(
  plugin: object & Lifecycle,
  options: RunStdioPluginOptions = {},
): Promise<void> {
  const name = options.name ?? process.env.SERA_PLUGIN_NAME ?? "unknown";
  const version = options.version ?? { major: 0, minor: 0, patch: 1 };
  const input = options.input ?? process.stdin;
  const output = options.output ?? process.stdout;
  const signals = options.signals ?? ["SIGTERM", "SIGINT"];

  await plugin.onStartup?.();

  let shuttingDown = false;
  const shutdown = async (exitCode = 0): Promise<void> => {
    if (shuttingDown) return;
    shuttingDown = true;
    try {
      await plugin.onShutdown?.();
    } catch (err) {
      process.stderr.write(`[sera-plugin-sdk] onShutdown error: ${String(err)}\n`);
    }
    process.exit(exitCode);
  };

  for (const sig of signals) {
    process.on(sig, () => {
      void shutdown(0);
    });
  }

  const rl = createInterface({ input, terminal: false });

  const send = (resp: JsonRpcResponse): void => {
    output.write(JSON.stringify(resp) + "\n");
  };

  rl.on("line", (line) => {
    const trimmed = line.trim();
    if (trimmed.length === 0) return;
    void handleLine(plugin, trimmed, send, { name, version });
  });

  rl.on("close", () => {
    void shutdown(0);
  });

  await once(rl, "close");
}

async function handleLine(
  plugin: object,
  line: string,
  send: (resp: JsonRpcResponse) => void,
  meta: { name: string; version: { major: number; minor: number; patch: number } },
): Promise<void> {
  let req: JsonRpcRequest;
  try {
    const parsed = JSON.parse(line) as unknown;
    if (
      typeof parsed !== "object" || parsed === null ||
      (parsed as { jsonrpc?: unknown }).jsonrpc !== "2.0" ||
      typeof (parsed as { method?: unknown }).method !== "string"
    ) {
      throw new ProtocolError("expected JSON-RPC 2.0 request");
    }
    req = parsed as JsonRpcRequest;
  } catch (err) {
    send({
      jsonrpc: "2.0",
      id: null,
      error: { code: -32700, message: `parse error: ${(err as Error).message}` },
    });
    return;
  }

  try {
    const result = await route(plugin, req.method, req.params, meta);
    send({ jsonrpc: "2.0", id: req.id, result });
  } catch (err) {
    send({
      jsonrpc: "2.0",
      id: req.id,
      error: toJsonRpcError(err),
    });
  }
}

async function route(
  plugin: object,
  method: string,
  params: unknown,
  meta: { name: string; version: { major: number; minor: number; patch: number } },
): Promise<unknown> {
  if (BUILTIN_METHODS.has(method)) {
    return handleBuiltin(plugin, method, meta);
  }
  return dispatch(plugin, method, params);
}

function handleBuiltin(
  plugin: object,
  method: string,
  meta: { name: string; version: { major: number; minor: number; patch: number } },
): unknown {
  switch (method) {
    case "Register":
      return {
        name: meta.name,
        version: meta.version,
        capabilities: advertisedCapabilities(plugin),
        protocol_version: PROTOCOL_VERSION,
      };
    case "Heartbeat":
      return { ok: true, ts: new Date().toISOString() };
    case "Deregister":
      return { ok: true };
    default:
      throw new ProtocolError(`unknown builtin method: ${method}`);
  }
}

function toJsonRpcError(err: unknown): { code: number; message: string; data?: unknown } {
  if (err instanceof PluginError) {
    return {
      code: -32000,
      message: err.message,
      data: { code: err.code, details: err.details },
    };
  }
  if (err instanceof Error) {
    return { code: -32603, message: err.message };
  }
  return { code: -32603, message: String(err) };
}
