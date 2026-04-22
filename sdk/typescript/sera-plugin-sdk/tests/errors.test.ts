import { describe, it, expect } from "vitest";
import {
  PluginError,
  RegistrationFailed,
  HealthCheckFailed,
  PluginNotFound,
  CircuitOpen,
  Unauthorized,
  ConnectionFailed,
  CapabilityNotImplemented,
} from "../src/errors.js";

describe("PluginError hierarchy", () => {
  it("RegistrationFailed extends PluginError and carries reason", () => {
    const e = new RegistrationFailed("duplicate name");
    expect(e).toBeInstanceOf(PluginError);
    expect(e).toBeInstanceOf(Error);
    expect(e.code).toBe("RegistrationFailed");
    expect(e.details).toEqual({ reason: "duplicate name" });
    expect(e.message).toContain("duplicate name");
  });

  it("HealthCheckFailed includes plugin name and reason", () => {
    const e = new HealthCheckFailed("lcm", "timeout");
    expect(e.details).toEqual({ name: "lcm", reason: "timeout" });
    expect(e.message).toContain("lcm");
    expect(e.message).toContain("timeout");
  });

  it("PluginNotFound carries name in details", () => {
    const e = new PluginNotFound("missing-plugin");
    expect(e.code).toBe("PluginNotFound");
    expect(e.details).toEqual({ name: "missing-plugin" });
  });

  it("CircuitOpen carries plugin name", () => {
    const e = new CircuitOpen("failing");
    expect(e.code).toBe("CircuitOpen");
    expect(e.details).toEqual({ name: "failing" });
  });

  it("Unauthorized carries reason", () => {
    const e = new Unauthorized("missing cert");
    expect(e.code).toBe("Unauthorized");
    expect(e.details).toEqual({ reason: "missing cert" });
  });

  it("ConnectionFailed carries endpoint and reason", () => {
    const e = new ConnectionFailed("localhost:9090", "refused");
    expect(e.details).toEqual({ endpoint: "localhost:9090", reason: "refused" });
  });

  it("CapabilityNotImplemented reports capability + method", () => {
    const e = new CapabilityNotImplemented("ContextEngine", "ingest");
    expect(e.details).toEqual({ capability: "ContextEngine", method: "ingest" });
  });

  it("every subclass preserves its .name for instanceof/console readability", () => {
    expect(new RegistrationFailed("x").name).toBe("RegistrationFailed");
    expect(new PluginNotFound("x").name).toBe("PluginNotFound");
    expect(new CircuitOpen("x").name).toBe("CircuitOpen");
  });
});
