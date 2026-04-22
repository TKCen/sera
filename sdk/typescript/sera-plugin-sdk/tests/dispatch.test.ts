import { describe, it, expect } from "vitest";
import {
  ContextEngine,
  ContextQuery,
  ContextDiagnostics,
  MemoryBackend,
} from "../src/capabilities/index.js";
import { advertisedCapabilities, detectCapabilities, dispatch } from "../src/dispatch.js";
import { CapabilityNotImplemented } from "../src/errors.js";

class FullCtxPlugin implements ContextEngine, ContextQuery, ContextDiagnostics {
  async ingest(): Promise<void> { /* no-op */ }
  async assemble(): Promise<{ messages: []; tokens_used: 0; truncated: false }> {
    return { messages: [], tokens_used: 0, truncated: false };
  }
  async search(): Promise<{ hits: [] }> { return { hits: [] }; }
  async expand(id: string): Promise<{ id: string; score: 1; content: string }> {
    return { id, score: 1, content: "" };
  }
  async status(session_id: string) {
    return { session_id, healthy: true, compacted_turns: 0, live_turns: 0 };
  }
  async doctor() {
    return { healthy: true, issues: [], remediations: [] };
  }
}

class MemoryPlugin implements MemoryBackend {
  async write(r: { id: string }) { return { id: r.id }; }
  async search() { return []; }
  async delete(): Promise<void> { /* no-op */ }
}

describe("capability detection", () => {
  it("identifies all three context capabilities on a combined plugin", () => {
    const caps = detectCapabilities(new FullCtxPlugin());
    expect(caps.ContextEngine).toBe(true);
    expect(caps.ContextQuery).toBe(true);
    expect(caps.ContextDiagnostics).toBe(true);
    expect(caps.MemoryBackend).toBe(false);
  });

  it("identifies MemoryBackend and disambiguates from ContextEngine", () => {
    const caps = detectCapabilities(new MemoryPlugin());
    expect(caps.MemoryBackend).toBe(true);
    expect(caps.ContextEngine).toBe(false);
  });

  it("advertisedCapabilities returns the PascalCase list", () => {
    const list = advertisedCapabilities(new FullCtxPlugin());
    expect(list).toContain("ContextEngine");
    expect(list).toContain("ContextQuery");
    expect(list).toContain("ContextDiagnostics");
    expect(list).not.toContain("MemoryBackend");
  });
});

describe("dispatch", () => {
  it("routes a known method to the plugin handler", async () => {
    const p = new FullCtxPlugin();
    const res = await dispatch(p, "doctor", undefined);
    expect(res).toEqual({ healthy: true, issues: [], remediations: [] });
  });

  it("throws CapabilityNotImplemented for unknown methods", async () => {
    const p = new MemoryPlugin();
    await expect(dispatch(p, "assemble", undefined)).rejects.toThrow(CapabilityNotImplemented);
  });
});
