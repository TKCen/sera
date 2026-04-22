import { existsSync } from "node:fs";
import { resolve } from "node:path";
import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";
import { advertisedCapabilities, dispatch } from "../dispatch.js";
import { PluginError, ProtocolError } from "../errors.js";
import { type Lifecycle } from "../lifecycle.js";
import { PROTOCOL_VERSION } from "../types.js";

export interface RunGrpcPluginOptions {
  /** Plugin name. Defaults to `process.env.SERA_PLUGIN_NAME ?? "unknown"`. */
  name?: string;
  version?: { major: number; minor: number; patch: number };
  /** host:port to bind. Default: 0.0.0.0:9090. */
  endpoint?: string;
  /** Path to the directory holding the plugin .proto files. */
  protoDir?: string;
  /** Credentials. Default: insecure (localhost/dev only — Tier 2/3 MUST set this). */
  credentials?: grpc.ServerCredentials;
  signals?: NodeJS.Signals[];
}

/**
 * Run a plugin over gRPC. Loads the proto descriptors at startup,
 * registers a generic service that dispatches incoming calls to the
 * plugin instance, and binds the server to `endpoint`.
 *
 * The returned promise resolves when the server shuts down (on SIGTERM
 * or a manual `server.tryShutdown` via the returned handle).
 *
 * NOTE: this is a thin ergonomic shim over `@grpc/grpc-js` +
 * `@grpc/proto-loader`. When `rust/proto/plugin/` is absent at SDK
 * build time, the generated stubs are empty — callers who actually
 * want gRPC must supply `protoDir` pointing at a checkout of the
 * canonical protos. See SPEC-plugins §5.1.
 */
export async function runGrpcPlugin(
  plugin: object & Lifecycle,
  options: RunGrpcPluginOptions = {},
): Promise<{ server: grpc.Server; shutdown: () => Promise<void> }> {
  const name = options.name ?? process.env.SERA_PLUGIN_NAME ?? "unknown";
  const version = options.version ?? { major: 0, minor: 0, patch: 1 };
  const endpoint = options.endpoint ?? "0.0.0.0:9090";
  const credentials = options.credentials ?? grpc.ServerCredentials.createInsecure();
  const signals = options.signals ?? ["SIGTERM", "SIGINT"];
  const protoDir = options.protoDir ?? resolveDefaultProtoDir();

  if (protoDir === null || !existsSync(protoDir)) {
    throw new ProtocolError(
      "no proto directory available — pass options.protoDir pointing at rust/proto/plugin/, " +
      "or set SERA_PROTO_DIR. gRPC transport requires proto descriptors.",
    );
  }

  const packageDef = protoLoader.loadSync(`${protoDir}/registry.proto`, {
    keepCase: true,
    longs: String,
    enums: String,
    defaults: true,
    oneofs: true,
    includeDirs: [protoDir],
  });
  const loaded = grpc.loadPackageDefinition(packageDef);

  await plugin.onStartup?.();

  const server = new grpc.Server();

  // Register a generic "CapabilityDispatch" handler. The wire-level service
  // binding is looked up from the loaded package. If the user's proto set
  // exposes a different service, they can post-process `server` before the
  // returned shutdown() is called.
  const service = findService(loaded, "PluginRegistry");
  if (service !== null) {
    server.addService(service, buildHandlers(plugin, { name, version }));
  }

  await new Promise<void>((resolveBind, rejectBind) => {
    server.bindAsync(endpoint, credentials, (err, _port) => {
      if (err) rejectBind(err); else resolveBind();
    });
  });

  const shutdown = async (): Promise<void> => {
    try {
      await plugin.onShutdown?.();
    } catch (err) {
      process.stderr.write(`[sera-plugin-sdk] onShutdown error: ${String(err)}\n`);
    }
    await new Promise<void>((r) => {
      server.tryShutdown(() => r());
    });
  };

  for (const sig of signals) {
    process.on(sig, () => {
      void shutdown();
    });
  }

  return { server, shutdown };
}

function resolveDefaultProtoDir(): string | null {
  if (process.env.SERA_PROTO_DIR) return process.env.SERA_PROTO_DIR;
  return null;
}

type ServiceDef = Parameters<grpc.Server["addService"]>[0];

function findService(
  pkg: grpc.GrpcObject | grpc.ServiceClientConstructor | grpc.ProtobufTypeDefinition,
  name: string,
): ServiceDef | null {
  const candidate = (pkg as Record<string, unknown>)[name];
  if (candidate && typeof candidate === "function" && "service" in candidate) {
    return (candidate as { service: ServiceDef }).service;
  }
  for (const value of Object.values(pkg as Record<string, unknown>)) {
    if (value && typeof value === "object") {
      const nested = findService(value as grpc.GrpcObject, name);
      if (nested) return nested;
    }
  }
  return null;
}

function buildHandlers(
  plugin: object,
  meta: { name: string; version: { major: number; minor: number; patch: number } },
): Record<string, grpc.handleUnaryCall<unknown, unknown>> {
  return {
    Register: (call, callback) => {
      callback(null, {
        name: meta.name,
        version: meta.version,
        capabilities: advertisedCapabilities(plugin),
        protocol_version: PROTOCOL_VERSION,
      });
    },
    Heartbeat: (call, callback) => {
      callback(null, { ok: true, ts: new Date().toISOString() });
    },
    Deregister: (call, callback) => {
      callback(null, { ok: true });
    },
    CapabilityDispatch: (call, callback) => {
      const req = call.request as { method?: string; params?: unknown };
      if (typeof req?.method !== "string") {
        callback({ code: grpc.status.INVALID_ARGUMENT, message: "missing method" });
        return;
      }
      dispatch(plugin, req.method, req.params).then(
        (result) => callback(null, { result }),
        (err) => {
          const message = err instanceof PluginError || err instanceof Error
            ? err.message
            : String(err);
          callback({ code: grpc.status.INTERNAL, message });
        },
      );
    },
  };
}
