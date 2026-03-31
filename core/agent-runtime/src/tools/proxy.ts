/**
 * Remote tool proxy — HTTP calls to sera-core for tools that
 * can't execute inside the agent container.
 */

import type { ChatMessage } from '../llmClient.js';
import { NotPermittedError } from './types.js';
import { truncateOutput } from './file-handlers.js';
import { parseJson } from '../json.js';
import { log } from '../logger.js';

const AGENT_ID = process.env['AGENT_INSTANCE_ID'] || process.env['AGENT_NAME'] || 'unknown';

/** Check if the sera-core proxy is available. */
export function isProxyAvailable(): boolean {
  return !!(process.env['SERA_CORE_URL'] && process.env['SERA_IDENTITY_TOKEN']);
}

/** Spawn a subagent via sera-core. */
export async function spawnSubagent(
  tier: number,
  role: string,
  task: string
): Promise<string> {
  if (tier === 1) {
    throw new NotPermittedError('spawn-subagent is not available for tier-1 agents');
  }
  if (!isProxyAvailable()) {
    return 'Error: Cannot spawn subagent — SERA_CORE_URL not configured';
  }

  const agentName = process.env['AGENT_NAME'] ?? 'unknown';
  const coreUrl = process.env['SERA_CORE_URL'] ?? '';
  const token = process.env['SERA_IDENTITY_TOKEN'] ?? '';

  try {
    const res = await fetch(`${coreUrl}/api/sandbox/subagent`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
      body: JSON.stringify({ agentName, subagentRole: role, task }),
      signal: AbortSignal.timeout(120_000),
    });

    const responseBody = await res.text();
    if (res.status === 403) return `Error: Permission denied — ${responseBody}`;
    if (!res.ok) return `Error: Subagent spawn failed (HTTP ${res.status}): ${responseBody}`;
    return responseBody;
  } catch (err) {
    return `Error: Subagent spawn failed: ${err instanceof Error ? err.message : String(err)}`;
  }
}

/** Run an ephemeral tool via sera-core. */
export async function runTool(
  tier: number,
  toolName: string,
  command: string,
  timeoutSeconds?: number
): Promise<string> {
  if (tier === 1) {
    throw new NotPermittedError('run-tool is not available for tier-1 agents');
  }
  if (!isProxyAvailable()) {
    return 'Error: Cannot run tool — SERA_CORE_URL not configured';
  }

  const agentName = process.env['AGENT_NAME'] ?? 'unknown';
  const coreUrl = process.env['SERA_CORE_URL'] ?? '';
  const token = process.env['SERA_IDENTITY_TOKEN'] ?? '';
  const timeoutMs = Math.min((timeoutSeconds ?? 120) + 10, 310) * 1000;

  try {
    const res = await fetch(`${coreUrl}/api/sandbox/tool`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
      body: JSON.stringify({
        agentName,
        toolName,
        command,
        ...(timeoutSeconds !== undefined ? { timeoutSeconds } : {}),
      }),
      signal: AbortSignal.timeout(timeoutMs),
    });

    const responseBody = await res.text();
    if (res.status === 403) return `Error: Permission denied — ${responseBody}`;
    if (!res.ok) return `Error: Tool execution failed (HTTP ${res.status}): ${responseBody}`;
    return responseBody;
  } catch (err) {
    return `Error: Tool execution failed: ${err instanceof Error ? err.message : String(err)}`;
  }
}

/** Forward a file tool call to sera-core's proxy endpoint. */
export async function executeProxiedTool(
  toolCallId: string,
  toolName: string,
  args: Record<string, unknown>,
  startMs: number
): Promise<ChatMessage> {
  try {
    const coreUrl = process.env['SERA_CORE_URL'] ?? '';
    const token = process.env['SERA_IDENTITY_TOKEN'] ?? '';

    const res = await fetch(`${coreUrl}/v1/tools/proxy`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
      body: JSON.stringify({ tool: toolName, args }),
      signal: AbortSignal.timeout(30_000),
    });

    const responseBody = await res.text();

    if (res.status === 403) {
      logInvocation(toolName, 'error', Date.now() - startMs);
      return {
        role: 'tool',
        tool_call_id: toolCallId,
        content: 'Error: No active grant for this path. Request filesystem access first.',
      };
    }

    if (!res.ok) {
      logInvocation(toolName, 'error', Date.now() - startMs);
      return {
        role: 'tool',
        tool_call_id: toolCallId,
        content: `Error: Proxy returned HTTP ${res.status}: ${responseBody}`,
      };
    }

    const parsed = parseJson(responseBody);
    const content =
      (parsed['result'] as string | undefined) ??
      (parsed['error'] as string | undefined) ??
      responseBody;
    logInvocation(toolName, 'success', Date.now() - startMs);
    return { role: 'tool', tool_call_id: toolCallId, content: truncateOutput(String(content)) };
  } catch (err) {
    const errorMsg = err instanceof Error ? err.message : String(err);
    logInvocation(toolName, 'error', Date.now() - startMs);
    return {
      role: 'tool',
      tool_call_id: toolCallId,
      content: `Error: Proxy call failed: ${errorMsg}`,
    };
  }
}

function logInvocation(toolName: string, status: 'success' | 'error', elapsedMs: number): void {
  log('debug', `tool=${toolName} agent=${AGENT_ID} status=${status} elapsed=${elapsedMs}ms`);
}
