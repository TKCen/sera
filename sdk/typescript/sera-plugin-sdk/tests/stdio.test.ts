import { describe, it, expect } from "vitest";
import { PassThrough } from "node:stream";
import { runStdioPlugin } from "../src/transport/stdio.js";
import type { ContextEngine } from "../src/capabilities/index.js";

/**
 * runStdioPlugin normally returns only when stdin EOFs or the process
 * signals shutdown — which in turn calls process.exit(). The tests use a
 * PassThrough pair as stdio substitutes so the loop runs inside Vitest
 * without exiting. We drive the input, collect the output, then end the
 * stream to let the call resolve.
 */

class StubEngine implements ContextEngine {
  ingested: unknown[] = [];
  async ingest(msg: unknown): Promise<void> {
    this.ingested.push(msg);
  }
  async assemble() {
    return { messages: [], tokens_used: 42, truncated: false };
  }
  startupCalls = 0;
  shutdownCalls = 0;
  async onStartup(): Promise<void> { this.startupCalls += 1; }
  async onShutdown(): Promise<void> { this.shutdownCalls += 1; }
}

function collectLines(output: NodeJS.ReadableStream): Promise<string[]> {
  return new Promise((resolve) => {
    const chunks: string[] = [];
    output.on("data", (c: Buffer) => chunks.push(c.toString("utf8")));
    output.on("end", () => {
      resolve(chunks.join("").split("\n").filter((l) => l.length > 0));
    });
  });
}

async function runWithScript(plugin: StubEngine, lines: string[]): Promise<string[]> {
  const input = new PassThrough();
  const output = new PassThrough();
  const collector = collectLines(output);

  // runStdioPlugin attaches signal handlers we don't want in tests — pass []
  // so no handlers are installed; the readline 'close' handler calls
  // process.exit(0), which would nuke the test runner. We patch it out
  // locally by stubbing process.exit for the duration of the call.
  const origExit = process.exit;
  // @ts-expect-error — intentional test stub
  process.exit = (() => { /* swallow */ }) as typeof process.exit;

  const done = runStdioPlugin(plugin, {
    name: "test-plugin",
    version: { major: 1, minor: 2, patch: 3 },
    input,
    output,
    signals: [],
  });

  for (const l of lines) input.write(l + "\n");
  input.end();

  try {
    await done;
  } finally {
    process.exit = origExit;
    output.end();
  }
  return collector;
}

describe("runStdioPlugin", () => {
  it("responds to Register with name, version, and capabilities", async () => {
    const plugin = new StubEngine();
    const out = await runWithScript(plugin, [
      JSON.stringify({ jsonrpc: "2.0", id: 1, method: "Register" }),
    ]);
    expect(out).toHaveLength(1);
    const resp = JSON.parse(out[0] ?? "{}") as { id: number; result: { name: string; capabilities: string[]; version: { major: number } } };
    expect(resp.id).toBe(1);
    expect(resp.result.name).toBe("test-plugin");
    expect(resp.result.version.major).toBe(1);
    expect(resp.result.capabilities).toContain("ContextEngine");
  });

  it("dispatches capability methods to the plugin", async () => {
    const plugin = new StubEngine();
    const out = await runWithScript(plugin, [
      JSON.stringify({ jsonrpc: "2.0", id: 7, method: "assemble", params: { session_id: "s", token_budget: 10 } }),
    ]);
    const resp = JSON.parse(out[0] ?? "{}") as { id: number; result: { tokens_used: number } };
    expect(resp.id).toBe(7);
    expect(resp.result.tokens_used).toBe(42);
  });

  it("returns a JSON-RPC error for unknown methods", async () => {
    const plugin = new StubEngine();
    const out = await runWithScript(plugin, [
      JSON.stringify({ jsonrpc: "2.0", id: 2, method: "doesNotExist" }),
    ]);
    const resp = JSON.parse(out[0] ?? "{}") as { error: { code: number; message: string } };
    expect(resp.error.code).toBe(-32000);
    expect(resp.error.message).toMatch(/doesNotExist/);
  });

  it("returns a parse error on malformed input without crashing", async () => {
    const plugin = new StubEngine();
    const out = await runWithScript(plugin, ["not-json"]);
    const resp = JSON.parse(out[0] ?? "{}") as { error: { code: number } };
    expect(resp.error.code).toBe(-32700);
  });

  it("invokes onStartup before any dispatch", async () => {
    const plugin = new StubEngine();
    await runWithScript(plugin, [
      JSON.stringify({ jsonrpc: "2.0", id: 1, method: "Heartbeat" }),
    ]);
    expect(plugin.startupCalls).toBe(1);
  });
});
