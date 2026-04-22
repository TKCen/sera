/**
 * Plugin SDK error hierarchy. Mirrors the Rust `PluginError` enum in
 * `rust/crates/sera-plugins/src/error.rs`. Every SDK-raised exception
 * extends `PluginError`, so plugin authors can catch a single base class
 * and forward structured details upstream.
 */
export class PluginError extends Error {
  readonly code: string;
  readonly details: Record<string, unknown>;

  constructor(code: string, message: string, details: Record<string, unknown> = {}) {
    super(message);
    this.name = "PluginError";
    this.code = code;
    this.details = details;
  }
}

export class RegistrationFailed extends PluginError {
  constructor(reason: string) {
    super("RegistrationFailed", `plugin registration failed: ${reason}`, { reason });
    this.name = "RegistrationFailed";
  }
}

export class HealthCheckFailed extends PluginError {
  constructor(name: string, reason: string) {
    super("HealthCheckFailed", `health check failed for plugin '${name}': ${reason}`, {
      name,
      reason,
    });
    this.name = "HealthCheckFailed";
  }
}

export class PluginNotFound extends PluginError {
  constructor(name: string) {
    super("PluginNotFound", `plugin not found: ${name}`, { name });
    this.name = "PluginNotFound";
  }
}

export class PluginUnhealthy extends PluginError {
  constructor(name: string) {
    super("PluginUnhealthy", `plugin '${name}' is unhealthy`, { name });
    this.name = "PluginUnhealthy";
  }
}

export class ManifestInvalid extends PluginError {
  constructor(reason: string) {
    super("ManifestInvalid", `manifest invalid: ${reason}`, { reason });
    this.name = "ManifestInvalid";
  }
}

export class CircuitOpen extends PluginError {
  constructor(name: string) {
    super("CircuitOpen", `circuit breaker open for plugin '${name}'`, { name });
    this.name = "CircuitOpen";
  }
}

export class Unauthorized extends PluginError {
  constructor(reason: string) {
    super("Unauthorized", `unauthorized: ${reason}`, { reason });
    this.name = "Unauthorized";
  }
}

export class ConnectionFailed extends PluginError {
  constructor(endpoint: string, reason: string) {
    super("ConnectionFailed", `connection failed to '${endpoint}': ${reason}`, {
      endpoint,
      reason,
    });
    this.name = "ConnectionFailed";
  }
}

export class CapabilityNotImplemented extends PluginError {
  constructor(capability: string, method: string) {
    super(
      "CapabilityNotImplemented",
      `plugin does not implement ${capability}.${method}`,
      { capability, method },
    );
    this.name = "CapabilityNotImplemented";
  }
}

export class ProtocolError extends PluginError {
  constructor(reason: string) {
    super("ProtocolError", `protocol error: ${reason}`, { reason });
    this.name = "ProtocolError";
  }
}
