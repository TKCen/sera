/**
 * Tool handler for code-eval.
 * Executes JavaScript/TypeScript code in an isolated context within the agent container.
 */

import { createContext, runInContext } from 'vm';

const DEFAULT_TIMEOUT_MS = 5000;
const MAX_TIMEOUT_MS = 30000;

/**
 * Handle code-eval tool call.
 */
export async function codeEval(
  code: string,
  language: string = 'javascript',
  timeout: number = DEFAULT_TIMEOUT_MS
): Promise<string> {
  const effectiveTimeout = Math.min(Math.max(1, timeout), MAX_TIMEOUT_MS);
  const start = Date.now();

  try {
    // ── Create a restricted sandbox context ─────────────────────────────
    // Strip access to process, require, Bun, filesystem APIs, etc.
    const sandbox: Record<string, any> = {
      console: {
        log: (...args: any[]) => {
          logs.push(args.map((a) => (typeof a === 'object' ? JSON.stringify(a) : String(a))).join(' '));
        },
        error: (...args: any[]) => {
          errors.push(args.map((a) => (typeof a === 'object' ? JSON.stringify(a) : String(a))).join(' '));
        },
      },
      // Standard JS globals only
      Math,
      Date,
      JSON,
      Array,
      Object,
      String,
      Number,
      Boolean,
      RegExp,
      Map,
      Set,
      Promise,
      Error,
      setTimeout,
      clearTimeout,
      // No filesystem or network access
    };

    const logs: string[] = [];
    const errors: string[] = [];
    const context = createContext(sandbox);

    // ── Execute the code ──────────────────────────────────────────────
    // Bun's 'vm' module behavior might vary slightly, but runInContext is the standard way.
    // For TypeScript, we'd need a transpiler (e.g. Bun.Transpiler or a library),
    // but for now, we'll assume basic JS-compatible TS or handle it as JS.
    const result = runInContext(code, context, {
      timeout: effectiveTimeout,
      filename: 'code-eval.js',
    });

    const elapsed = Date.now() - start;
    const output = [
      logs.length > 0 ? `Stdout:\n${logs.join('\n')}` : '',
      errors.length > 0 ? `Stderr:\n${errors.join('\n')}` : '',
      result !== undefined ? `Result: ${JSON.stringify(result, null, 2)}` : '',
    ]
      .filter((s) => s.length > 0)
      .join('\n\n');

    return `Execution successful (${elapsed}ms):\n\n${output || '(no output)'}`;
  } catch (err: unknown) {
    const elapsed = Date.now() - start;
    const errorMsg = err instanceof Error ? err.stack || err.message : String(err);
    return `Execution failed after ${elapsed}ms:\n\n${errorMsg}`;
  }
}
