/**
 * RuntimeToolExecutor — dispatches tool calls to the appropriate handler.
 *
 * Local tools (file, shell) execute natively in the container.
 * Remote tools (subagent, run-tool, proxy) call sera-core via HTTP.
 */

import type { ChatMessage, ToolCall, ToolDefinition } from '../llmClient.js';
import { parseJson } from '../json.js';
import {
  repairToolArguments,
  sanitizeToolName,
  ToolArgumentParseError,
} from '../toolArgumentRepair.js';
import { log } from '../logger.js';
import { PermissionDeniedError, AGENT_ID } from './types.js';
import { BUILTIN_TOOLS } from './definitions.js';
import {
  imageView,
  fileRead,
  fileWrite,
  fileList,
  fileDelete,
  truncateOutput,
} from './file-handlers.js';
import { globFiles, grepFiles, readFilePartial } from './search-handlers.js';
import { pdfRead } from './pdf-handler.js';
import { codeEval } from './code-handler.js';
import { httpRequest } from './http-handler.js';
import type { RuntimeManifest } from '../manifest.js';
import { shellExec, shellExecStreaming, checkShellPathRestriction } from './shell-handler.js';
import { webFetchStreaming } from './web-handler.js';
import type { ToolOutputCallback } from '../centrifugo.js';
import { spawnSubagent, runTool, executeProxiedTool, isProxyAvailable } from './proxy.js';
import { HookRunner } from './hooks.js';
import { skillSearch } from './skill-handler.js';

/** Result of executing a single tool call, including repair metadata. */
export interface ToolExecutionResult {
  message: ChatMessage;
  toolName: string;
  argRepaired: boolean;
  repairStrategy: string | null;
}

/** Local tool names that are handled natively in the container. */
const LOCAL_TOOLS = new Set([
  'file-read',
  'file-write',
  'image-view',
  'pdf-read',
  'code-eval',
  'http-request',
  'file-list',
  'file-delete',
  'read_file',
  'glob',
  'grep',
  'shell-exec',
  'web-fetch',
  'spawn-subagent',
  'run-tool',
  'tool-search',
  'skill-search',
]);

/** Tools that modify state — executed with mutual exclusion to prevent races. */
export const WRITE_TOOLS = new Set(['file-write', 'file-delete', 'shell-exec']);

const DEFAULT_MAX_TOOL_CONCURRENCY = 4;

// ── Interface ─────────────────────────────────────────────────────────────────

export interface IToolExecutor {
  getToolDefinitions(allowedTools?: string[]): ToolDefinition[];
  executeToolCalls(
    toolCalls: ToolCall[],
    onOutput?: ToolOutputCallback
  ): Promise<ToolExecutionResult[]>;
}

// ── Semaphore for concurrency control ────────────────────────────────────────

class Semaphore {
  private permits: number;
  private queue: Array<() => void> = [];
  constructor(permits: number) {
    this.permits = permits;
  }
  async acquire(): Promise<void> {
    if (this.permits > 0) {
      this.permits--;
      return;
    }
    return new Promise<void>((resolve) => {
      this.queue.push(resolve);
    });
  }
  release(): void {
    const next = this.queue.shift();
    if (next) {
      next();
    } else {
      this.permits++;
    }
  }
}

export class RuntimeToolExecutor implements IToolExecutor {
  private workspacePath: string;
  private tier: number;
  private manifest?: RuntimeManifest;
  /** Remote tools fetched from core's catalog. */
  private remoteCatalog: ToolDefinition[] = [];
  /** Mutex for write tools — at most 1 write tool at a time. */
  private writeMutex = new Semaphore(1);
  /** Max concurrent tool executions. */
  private concurrency: number;
  private hookRunner: HookRunner;

  constructor(workspacePath: string = '/workspace', tier?: number, manifest?: RuntimeManifest) {
    this.workspacePath = workspacePath;
    this.tier = tier ?? (process.env['AGENT_TIER'] ? parseInt(process.env['AGENT_TIER'], 10) : 2);
    this.manifest = manifest;
    const envConcurrency = process.env['MAX_TOOL_CONCURRENCY'];
    this.concurrency = envConcurrency ? parseInt(envConcurrency, 10) : DEFAULT_MAX_TOOL_CONCURRENCY;
    this.hookRunner = new HookRunner(manifest?.tools?.hooks || []);
  }

  /**
   * Fetch the dynamic tool catalog from core (Story 7.6).
   * Populates remoteCatalog with tools the agent can use but
   * that execute on the core side via POST /v1/tools/invoke.
   * Falls back to BUILTIN_TOOLS only if the catalog is unreachable.
   */
  async fetchCatalog(): Promise<void> {
    if (!isProxyAvailable()) {
      log('info', 'Tool catalog: core not available, using local tools only');
      return;
    }

    const coreUrl = process.env['SERA_CORE_URL'] ?? '';
    const token = process.env['SERA_IDENTITY_TOKEN'] ?? '';
    const agentId = process.env['AGENT_INSTANCE_ID'] ?? AGENT_ID;

    try {
      const res = await fetch(
        `${coreUrl}/v1/tools/catalog?agentId=${encodeURIComponent(agentId)}`,
        {
          headers: { Authorization: `Bearer ${token}` },
          signal: AbortSignal.timeout(10_000),
        }
      );

      if (!res.ok) {
        log('warn', `Tool catalog fetch failed (HTTP ${res.status}), using local tools only`);
        return;
      }

      const catalog = (await res.json()) as Array<ToolDefinition & { executionMode?: string }>;
      this.remoteCatalog = catalog.filter(
        (t) => t.executionMode === 'remote' && !LOCAL_TOOLS.has(t.function.name)
      );

      const localCount = BUILTIN_TOOLS.length;
      const remoteCount = this.remoteCatalog.length;
      log(
        'info',
        `Tool catalog: ${localCount} local + ${remoteCount} remote = ${localCount + remoteCount} total tools`
      );
    } catch (err) {
      log('warn', `Tool catalog fetch error: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  /** Get tool definitions that the LLM can call, filtered by manifest. */
  getToolDefinitions(allowedTools?: string[]): ToolDefinition[] {
    const allTools = [...BUILTIN_TOOLS, ...this.remoteCatalog];
    if (!allowedTools || allowedTools.length === 0) {
      return allTools;
    }
    return allTools.filter(
      (t) =>
        allowedTools.includes(t.function.name) ||
        this.remoteCatalog.some((r) => r.function.name === t.function.name)
    );
  }

  /** Execute a single tool call and return a tool-role ChatMessage with repair metadata. */
  async executeTool(
    toolCall: ToolCall,
    onOutput?: ToolOutputCallback,
    sessionId?: string
  ): Promise<ToolExecutionResult> {
    const { id, function: fn } = toolCall;
    const toolName = sanitizeToolName(fn.name);
    const start = Date.now();
    const agentInstanceId = process.env['AGENT_INSTANCE_ID'] || AGENT_ID;
    let params: Record<string, unknown> = {};

    try {
      // ── Built-in: Capability Check ────────────────────────────────────────
      const allowed = this.manifest?.tools?.allowed || ['*'];
      const denied = this.manifest?.tools?.denied || [];
      const isAllowed =
        (allowed.includes('*') || allowed.includes(toolName)) && !denied.includes(toolName);

      if (!isAllowed) {
        log(
          'warn',
          `Tool execution denied: ${toolName} not permitted for agent ${agentInstanceId}`
        );
        return {
          message: {
            role: 'tool',
            tool_call_id: id,
            content: `Error: tool_not_permitted: Access to tool "${toolName}" is denied by agent manifest.`,
          },
          toolName,
          argRepaired: false,
          repairStrategy: null,
        };
      }

      let argRepaired = false;
      let repairStrategy: string | null = null;
      try {
        const repair = repairToolArguments(fn.arguments || '{}');
        params = repair.parsed;
        argRepaired = repair.repaired;
        repairStrategy = repair.strategy;
      } catch (repairErr) {
        const result =
          repairErr instanceof ToolArgumentParseError
            ? `Error: Failed to parse tool arguments after repair attempts: ${fn.arguments}`
            : `Error: Failed to parse tool arguments as JSON: ${fn.arguments}`;
        this.logInvocation(toolName, 'error', Date.now() - start);
        return {
          message: { role: 'tool', tool_call_id: id, content: result },
          toolName,
          argRepaired: false,
          repairStrategy: null,
        };
      }

      // ── beforeToolCall Hooks ──────────────────────────────────────────────
      const beforeResult = await this.hookRunner.beforeToolCall({
        toolName,
        args: params,
        agentName: this.manifest?.metadata?.name || 'unknown',
        agentInstanceId,
        tier: this.tier,
      });

      if (beforeResult.status === 'warn' && beforeResult.message) {
        log('warn', `Hook warning (beforeToolCall): ${beforeResult.message}`);
      }

      if (beforeResult.status === 'deny') {
        return {
          message: {
            role: 'tool',
            tool_call_id: id,
            content: `Error: tool_denied: ${beforeResult.message || 'Execution denied by hook.'}`,
          },
          toolName,
          argRepaired,
          repairStrategy,
        };
      }

      if (beforeResult.modifiedArgs) {
        params = beforeResult.modifiedArgs;
      }

      let result: string;

      switch (toolName) {
        case 'image-view':
          result = imageView(
            this.workspacePath,
            params['path'] as string,
            params['prompt'] as string | undefined
          );
          break;
        case 'pdf-read':
          result = await pdfRead(
            this.workspacePath,
            params['path'] as string,
            params['pages'] as string | undefined,
            params['format'] as 'text' | 'markdown' | undefined
          );
          break;
        case 'code-eval':
          result = await codeEval(
            params['code'] as string,
            params['language'] as 'javascript' | 'typescript' | undefined,
            params['timeout'] as number | undefined
          );
          break;
        case 'http-request':
          result = await httpRequest(
            params['url'] as string,
            params['method'] as 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE' | undefined,
            params['headers'] as Record<string, string> | undefined,
            params['body'] as string | undefined,
            params['timeout'] as number | undefined
          );
          break;
        case 'file-read':
          result = fileRead(this.workspacePath, params['path'] as string, onOutput, id);
          break;
        case 'read_file':
          result = await readFilePartial(
            this.workspacePath,
            params['path'] as string,
            params['offset'] as number | undefined,
            params['limit'] as number | undefined,
            onOutput,
            id
          );
          break;
        case 'glob':
          result = globFiles(this.workspacePath, params['pattern'] as string);
          break;
        case 'grep':
          result = grepFiles(
            this.workspacePath,
            params['pattern'] as string,
            params['path'] as string | undefined,
            params['mode'] as any
          );
          break;
        case 'file-write':
          result = fileWrite(
            this.workspacePath,
            params['path'] as string,
            params['content'] as string
          );
          break;
        case 'file-list':
          result = fileList(this.workspacePath, params['path'] as string | undefined);
          break;
        case 'file-delete':
          result = fileDelete(
            this.workspacePath,
            params['path'] as string,
            params['recursive'] as boolean | undefined
          );
          break;
        case 'web-fetch':
          if (onOutput) {
            result = await webFetchStreaming(params['url'] as string, onOutput, id);
          } else {
            // Re-use streaming logic even for sync-style calls for consistency
            result = await webFetchStreaming(params['url'] as string, () => {}, id);
          }
          break;
        case 'shell-exec': {
          const outsidePath = checkShellPathRestriction(
            this.workspacePath,
            params['command'] as string
          );
          if (outsidePath && isProxyAvailable()) {
            result = JSON.stringify({
              error: 'path_requires_restart',
              hint: `Path "${outsidePath}" is outside /workspace. Shell access to dynamically granted paths requires a persistent grant and container restart.`,
            });
          } else if (onOutput) {
            result = await shellExecStreaming(
              this.workspacePath,
              this.tier,
              params['command'] as string,
              params['timeout_ms'] as number | undefined,
              onOutput,
              id
            );
          } else {
            result = shellExec(
              this.workspacePath,
              this.tier,
              params['command'] as string,
              params['timeout_ms'] as number | undefined
            );
          }
          break;
        }
        case 'spawn-subagent':
          result = await spawnSubagent(
            this.tier,
            params['role'] as string,
            params['task'] as string
          );
          break;
        case 'run-tool':
          result = await runTool(
            this.tier,
            params['tool_name'] as string,
            params['command'] as string,
            params['timeout_seconds'] as number | undefined
          );
          break;
        case 'tool-search':
          result = this.searchTools(params['query'] as string);
          break;
        case 'skill-search':
          result = await skillSearch(params);
          break;
        default:
          // Route to core's invoke endpoint for remote tools (ADR-001)
          if (isProxyAvailable() && this.remoteCatalog.some((t) => t.function.name === toolName)) {
            result = await this.invokeRemoteTool(toolName, params);
          } else {
            result = `Error: Unknown tool "${toolName}"`;
          }
      }

      const durationMs = Date.now() - start;
      this.logInvocation(toolName, 'success', durationMs);

      // ── afterToolCall Hooks ───────────────────────────────────────────────
      const afterResult = await this.hookRunner.afterToolCall({
        toolName,
        args: params,
        result,
        isError: false,
        agentName: this.manifest?.metadata?.name || 'unknown',
        agentInstanceId,
        tier: this.tier,
      });

      if (afterResult.status === 'warn' && afterResult.message) {
        log('warn', `Hook warning (afterToolCall): ${afterResult.message}`);
      }

      if (afterResult.modifiedResult !== undefined) {
        result = afterResult.modifiedResult;
      }

      const finalResult = truncateOutput(result);

      if (onOutput && toolName !== 'shell-exec' && toolName !== 'web-fetch') {
        onOutput({
          toolCallId: id,
          toolName,
          result: result.substring(0, 500),
          duration: durationMs,
          error: result.startsWith('Error:'),
          timestamp: new Date().toISOString(),
        });
      }

      if (this.manifest?.logging?.commands && sessionId) {
        this.sendCommandLog(sessionId, toolName, params, finalResult, durationMs, 'success');
      }

      return {
        message: { role: 'tool', tool_call_id: id, content: finalResult },
        toolName,
        argRepaired,
        repairStrategy,
      };
    } catch (err) {
      const durationMs = Date.now() - start;
      // Story 3.10: If a file tool path is outside workspace, try proxying
      if (err instanceof PermissionDeniedError && isProxyAvailable()) {
        const fileTools = new Set(['file-read', 'file-write', 'file-list', 'file-delete']);
        if (fileTools.has(toolName)) {
          const params = parseJson(fn.arguments || '{}');
          const proxied = await executeProxiedTool(id, toolName, params, start);
          return { message: proxied, toolName, argRepaired: false, repairStrategy: null };
        }
      }

      const errorMsg = err instanceof Error ? err.message : String(err);
      this.logInvocation(toolName, 'error', durationMs);

      // ── afterToolCall Hooks (Error Case) ──────────────────────────────────
      const afterResult = await this.hookRunner
        .afterToolCall({
          toolName,
          args: params || {},
          result: errorMsg,
          isError: true,
          agentName: this.manifest?.metadata?.name || 'unknown',
          agentInstanceId,
          tier: this.tier,
        })
        .catch(() => ({ status: 'allow' as const }));

      let finalError = errorMsg;
      if (afterResult.modifiedResult !== undefined) {
        finalError = afterResult.modifiedResult;
      }

      if (this.manifest?.logging?.commands && sessionId) {
        let parsedArgs = params || {};
        if (!params) {
          try {
            parsedArgs = JSON.parse(fn.arguments || '{}');
          } catch {
            /* ignore */
          }
        }
        this.sendCommandLog(sessionId, toolName, parsedArgs, finalError, durationMs, 'error');
      }

      return {
        message: { role: 'tool', tool_call_id: id, content: `Error: ${finalError}` },
        toolName,
        argRepaired: false,
        repairStrategy: null,
      };
    }
  }

  /**
   * Execute tool calls with concurrency. Write tools are serialized via a mutex;
   * read-only tools run in parallel up to the configured concurrency limit.
   * Results maintain the same order as the input tool calls.
   */
  async executeToolCalls(
    toolCalls: ToolCall[],
    onOutput?: ToolOutputCallback,
    sessionId?: string
  ): Promise<ToolExecutionResult[]> {
    // Fast path: single call, no concurrency overhead
    if (toolCalls.length <= 1) {
      const results: ToolExecutionResult[] = [];
      for (const tc of toolCalls) {
        results.push(await this.executeTool(tc, onOutput, sessionId));
      }
      return results;
    }

    const concurrencySem = new Semaphore(this.concurrency);
    const results = new Array<ToolExecutionResult>(toolCalls.length);

    const tasks = toolCalls.map(async (tc, index) => {
      await concurrencySem.acquire();
      try {
        const toolName = sanitizeToolName(tc.function.name);
        const isWrite = WRITE_TOOLS.has(toolName);
        if (isWrite) await this.writeMutex.acquire();
        try {
          results[index] = await this.executeTool(tc, onOutput, sessionId);
        } finally {
          if (isWrite) this.writeMutex.release();
        }
      } finally {
        concurrencySem.release();
      }
    });

    await Promise.all(tasks);
    return results;
  }

  /**
   * Search the combined tool catalog (local + remote) by query string.
   * Matches against tool name and description (case-insensitive).
   */
  private searchTools(query: string): string {
    const q = query.toLowerCase();
    const allTools = [...BUILTIN_TOOLS, ...this.remoteCatalog];
    const matches = allTools.filter((t) => {
      const name = t.function.name.toLowerCase();
      const desc = t.function.description.toLowerCase();
      return (
        name.includes(q) ||
        desc.includes(q) ||
        q.split(/\s+/).some((word) => desc.includes(word) || name.includes(word))
      );
    });

    if (matches.length === 0) {
      return `No tools found matching "${query}". Available tool categories: file operations, shell commands, web search/fetch, knowledge store/query, delegation, scheduling.`;
    }

    const results = matches.map((t) => `- **${t.function.name}**: ${t.function.description}`);
    return `Found ${matches.length} matching tool(s):\n${results.join('\n')}\n\nUse these tools by calling them directly via function calling.`;
  }

  /** Call core's POST /v1/tools/invoke for a remote tool. */
  private async invokeRemoteTool(
    toolName: string,
    params: Record<string, unknown>
  ): Promise<string> {
    const coreUrl = process.env['SERA_CORE_URL'] ?? '';
    const token = process.env['SERA_IDENTITY_TOKEN'] ?? '';

    try {
      const res = await fetch(`${coreUrl}/v1/tools/invoke`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ tool: toolName, params }),
        signal: AbortSignal.timeout(60_000),
      });

      const body = await res.text();
      if (!res.ok) return `Error: Remote tool ${toolName} failed (HTTP ${res.status}): ${body}`;

      try {
        const parsed = parseJson(body);
        if (parsed['error']) return `Error: ${parsed['error']}`;
        if (parsed['data']) return JSON.stringify(parsed['data'], null, 2);
        return body;
      } catch {
        return body;
      }
    } catch (err) {
      return `Error: Remote invoke failed: ${err instanceof Error ? err.message : String(err)}`;
    }
  }

  private logInvocation(toolName: string, status: 'success' | 'error', elapsedMs: number): void {
    log('debug', `tool=${toolName} agent=${AGENT_ID} status=${status} elapsed=${elapsedMs}ms`);
  }

  /** Send tool invocation details to core for debugging (Story 5.10) */
  private async sendCommandLog(
    sessionId: string,
    toolName: string,
    args: Record<string, unknown>,
    result: string,
    durationMs: number,
    status: 'success' | 'error'
  ): Promise<void> {
    const coreUrl = process.env['SERA_CORE_URL'] ?? '';
    const token = process.env['SERA_IDENTITY_TOKEN'] ?? '';
    const agentId = process.env['AGENT_INSTANCE_ID'] ?? AGENT_ID;

    // Truncate result to 2KB
    const MAX_LOG_RESULT_BYTES = 2048;
    const truncatedResult =
      result.length > MAX_LOG_RESULT_BYTES
        ? result.substring(0, MAX_LOG_RESULT_BYTES) + '... [TRUNCATED]'
        : result;

    try {
      await fetch(`${coreUrl}/api/agents/${encodeURIComponent(agentId)}/command-logs`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify({
          sessionId,
          toolName,
          arguments: sanitizeArgs(args),
          result: truncatedResult,
          durationMs,
          status,
        }),
      });
    } catch (err) {
      log(
        'warn',
        `Failed to send command log to core: ${err instanceof Error ? err.message : String(err)}`
      );
    }
  }
}

const SECRET_ARG_KEYS = new Set([
  'token',
  'key',
  'secret',
  'password',
  'api_key',
  'apikey',
  'auth',
  'credential',
]);

/** Remove secret-looking values from tool arguments before logging. */
function sanitizeArgs(args: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(args)) {
    if (SECRET_ARG_KEYS.has(k.toLowerCase())) {
      out[k] = '[REDACTED]';
    } else {
      out[k] = v;
    }
  }
  return out;
}
