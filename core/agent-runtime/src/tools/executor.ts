/**
 * RuntimeToolExecutor — dispatches tool calls to the appropriate handler.
 *
 * Local tools (file, shell) execute natively in the container.
 * Remote tools (subagent, run-tool, proxy) call sera-core via HTTP.
 */

import type { ChatMessage, ToolCall, ToolDefinition } from '../llmClient.js';
import { parseJson } from '../json.js';
import { log } from '../logger.js';
import { PermissionDeniedError, AGENT_ID } from './types.js';
import { BUILTIN_TOOLS } from './definitions.js';
import { fileRead, fileWrite, fileList, fileDelete, truncateOutput } from './file-handlers.js';
import { shellExec, checkShellPathRestriction } from './shell-handler.js';
import { spawnSubagent, runTool, executeProxiedTool, isProxyAvailable } from './proxy.js';

/** Local tool names that are handled natively in the container. */
const LOCAL_TOOLS = new Set([
  'file-read', 'file-write', 'file-list', 'file-delete',
  'shell-exec', 'spawn-subagent', 'run-tool',
]);

export class RuntimeToolExecutor {
  private workspacePath: string;
  private tier: number;
  /** Remote tools fetched from core's catalog. */
  private remoteCatalog: ToolDefinition[] = [];

  constructor(workspacePath: string = '/workspace', tier?: number) {
    this.workspacePath = workspacePath;
    this.tier = tier ?? (process.env['AGENT_TIER'] ? parseInt(process.env['AGENT_TIER'], 10) : 2);
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
      const res = await fetch(`${coreUrl}/v1/tools/catalog?agentId=${encodeURIComponent(agentId)}`, {
        headers: { Authorization: `Bearer ${token}` },
        signal: AbortSignal.timeout(10_000),
      });

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
      log('info', `Tool catalog: ${localCount} local + ${remoteCount} remote = ${localCount + remoteCount} total tools`);
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
    return allTools.filter((t) => allowedTools.includes(t.function.name));
  }

  /** Execute a single tool call and return a tool-role ChatMessage. */
  async executeTool(toolCall: ToolCall): Promise<ChatMessage> {
    const { id, function: fn } = toolCall;
    const toolName = fn.name;
    const start = Date.now();

    try {
      let params: Record<string, unknown>;
      try {
        params = parseJson(fn.arguments || '{}');
      } catch {
        const result = `Error: Failed to parse tool arguments as JSON: ${fn.arguments}`;
        this.logInvocation(toolName, 'error', Date.now() - start);
        return { role: 'tool', tool_call_id: id, content: result };
      }

      let result: string;

      switch (toolName) {
        case 'file-read':
          result = fileRead(this.workspacePath, params['path'] as string);
          break;
        case 'file-write':
          result = fileWrite(this.workspacePath, params['path'] as string, params['content'] as string);
          break;
        case 'file-list':
          result = fileList(this.workspacePath, params['path'] as string | undefined);
          break;
        case 'file-delete':
          result = fileDelete(this.workspacePath, params['path'] as string, params['recursive'] as boolean | undefined);
          break;
        case 'shell-exec': {
          const outsidePath = checkShellPathRestriction(this.workspacePath, params['command'] as string);
          if (outsidePath && isProxyAvailable()) {
            result = JSON.stringify({
              error: 'path_requires_restart',
              hint: `Path "${outsidePath}" is outside /workspace. Shell access to dynamically granted paths requires a persistent grant and container restart.`,
            });
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
          result = await spawnSubagent(this.tier, params['role'] as string, params['task'] as string);
          break;
        case 'run-tool':
          result = await runTool(
            this.tier,
            params['tool_name'] as string,
            params['command'] as string,
            params['timeout_seconds'] as number | undefined
          );
          break;
        default:
          // Route to core's invoke endpoint for remote tools (ADR-001)
          if (isProxyAvailable() && this.remoteCatalog.some((t) => t.function.name === toolName)) {
            result = await this.invokeRemoteTool(toolName, params);
          } else {
            result = `Error: Unknown tool "${toolName}"`;
          }
      }

      this.logInvocation(toolName, 'success', Date.now() - start);
      return { role: 'tool', tool_call_id: id, content: truncateOutput(result) };
    } catch (err) {
      // Story 3.10: If a file tool path is outside workspace, try proxying
      if (err instanceof PermissionDeniedError && isProxyAvailable()) {
        const fileTools = new Set(['file-read', 'file-write', 'file-list', 'file-delete']);
        if (fileTools.has(toolName)) {
          const params = parseJson(fn.arguments || '{}');
          return await executeProxiedTool(id, toolName, params, start);
        }
      }

      const errorMsg = err instanceof Error ? err.message : String(err);
      this.logInvocation(toolName, 'error', Date.now() - start);
      return { role: 'tool', tool_call_id: id, content: `Error: ${errorMsg}` };
    }
  }

  /** Execute multiple tool calls sequentially (order matters for state). */
  async executeToolCalls(toolCalls: ToolCall[]): Promise<ChatMessage[]> {
    const results: ChatMessage[] = [];
    for (const tc of toolCalls) {
      results.push(await this.executeTool(tc));
    }
    return results;
  }

  /** Call core's POST /v1/tools/invoke for a remote tool. */
  private async invokeRemoteTool(toolName: string, params: Record<string, unknown>): Promise<string> {
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
}
