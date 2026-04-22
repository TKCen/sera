import { z } from "zod";

/**
 * Capability discriminator — mirrors `PluginCapability` in
 * `rust/crates/sera-plugins/src/types.rs`. String values are the
 * serde-PascalCase names used on the wire.
 */
export const PluginCapabilityName = {
  MemoryBackend: "MemoryBackend",
  ToolExecutor: "ToolExecutor",
  ContextEngine: "ContextEngine",
  SandboxProvider: "SandboxProvider",
  AuthProvider: "AuthProvider",
  SecretProvider: "SecretProvider",
  RealtimeBackend: "RealtimeBackend",
} as const;

export type PluginCapabilityName =
  (typeof PluginCapabilityName)[keyof typeof PluginCapabilityName];

export const PluginVersionSchema = z.object({
  major: z.number().int().nonnegative(),
  minor: z.number().int().nonnegative(),
  patch: z.number().int().nonnegative(),
});
export type PluginVersion = z.infer<typeof PluginVersionSchema>;

export const TlsConfigSchema = z.object({
  ca_cert: z.string(),
  client_cert: z.string(),
  client_key: z.string(),
});
export type TlsConfig = z.infer<typeof TlsConfigSchema>;

export const GrpcTransportConfigSchema = z.object({
  endpoint: z.string(),
  tls: TlsConfigSchema.optional(),
});
export type GrpcTransportConfig = z.infer<typeof GrpcTransportConfigSchema>;

export const StdioTransportConfigSchema = z.object({
  command: z.array(z.string()).min(1),
  env: z.record(z.string()).default({}),
});
export type StdioTransportConfig = z.infer<typeof StdioTransportConfigSchema>;

export type PluginTransport =
  | { transport: "grpc"; grpc: GrpcTransportConfig }
  | { transport: "stdio"; stdio: StdioTransportConfig };

export const PluginRegistrationSchema = z.object({
  name: z.string().min(1),
  version: PluginVersionSchema,
  capabilities: z.array(z.string()).min(1),
  health_check_interval_secs: z.number().int().positive().default(30),
});
export type PluginRegistration = z.infer<typeof PluginRegistrationSchema>;

/**
 * Protocol version the SDK speaks. Bumps when the proto / JSON-Schema wire
 * contract changes (see SPEC-plugins §5.3 — SDKs release independently but
 * agree on this number).
 */
export const PROTOCOL_VERSION = "0.1.0";
