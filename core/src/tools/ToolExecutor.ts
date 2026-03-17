/**
 * ToolExecutor — bridges the SkillRegistry into the OpenAI tool-calling protocol.
 *
 * Converts skills available to an agent into OpenAI-format tool definitions,
 * executes tool calls via the SkillRegistry, and handles timeout + truncation.
 */

import type { AgentManifest } from '../agents/manifest/types.js';
import type { SkillRegistry } from '../skills/SkillRegistry.js';
import type { SkillInfo, SkillParameter } from '../skills/types.js';
import type { ToolDefinition, ToolCall } from '../lib/llm/types.js';
import type { ChatMessage } from '../agents/types.js';
import { Logger } from '../lib/logger.js';
import { parseJson } from '../lib/json.js';

const logger = new Logger('ToolExecutor');

/** Maximum characters in a single tool result before truncation. */
const MAX_RESULT_LENGTH = 50_000;

/** Default per-tool timeout in milliseconds. */
const DEFAULT_TOOL_TIMEOUT_MS = 60_000;

export class ToolExecutor {
  constructor(
    private readonly skillRegistry: SkillRegistry,
    private readonly sandboxManager?: import('../sandbox/SandboxManager.js').SandboxManager,
  ) {}

  // ── Tool Definitions ──────────────────────────────────────────────────────

  /**
   * Convert the skills available to an agent into OpenAI-format tool definitions.
   * These are passed to the LLM so it can request tool calls.
   */
  getToolDefinitions(manifest: AgentManifest): ToolDefinition[] {
    const skills = this.skillRegistry.listForAgent(manifest);
    return skills.map((skill) => ToolExecutor.skillToToolDef(skill));
  }

  // ── Execution ─────────────────────────────────────────────────────────────

  /**
   * Execute a single tool call. Returns a tool-role ChatMessage with the result.
   */
  async executeTool(
    toolCall: ToolCall,
    manifest: AgentManifest,
    agentInstanceId?: string,
    containerId?: string,
  ): Promise<ChatMessage> {
    const { id, function: fn } = toolCall;
    const skillId = fn.name;
    try {
      // Build AgentContext from Manifest
      const context: import('../skills/types.js').AgentContext = {
        agentName: manifest.metadata.name,
        workspacePath: manifest.workspace?.path || `workspaces/${manifest.metadata.name}`,
        tier: manifest.metadata.tier,
        agentInstanceId,
        containerId,
        sandboxManager: this.sandboxManager,
      };

      // Parse arguments
      let params: Record<string, unknown>;
      try {
        params = fn.arguments ? parseJson(fn.arguments) as Record<string, unknown> : {};
      } catch {
        return {
          role: 'tool',
          tool_call_id: id,
          content: `Error: Failed to parse tool arguments as JSON: ${fn.arguments}`,
        };
      }

      // Execute with timeout
      const result = await Promise.race([
        this.skillRegistry.invoke(skillId, params, context),
        ToolExecutor.timeout(DEFAULT_TOOL_TIMEOUT_MS, skillId),
      ]);

      // Format result
      let content: string;
      if (result.success) {
        content = typeof result.data === 'string'
          ? result.data
          : JSON.stringify(result.data, null, 2);
      } else {
        content = `Error: ${result.error ?? 'Unknown error'}`;
      }

      // Truncate
      content = ToolExecutor.truncate(content);

      return {
        role: 'tool',
        tool_call_id: id,
        content,
      };
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      logger.error(`Tool execution error for "${skillId}":`, err);
      return {
        role: 'tool',
        tool_call_id: id,
        content: `Error: ${errorMsg}`,
      };
    }
  }

  /**
   * Execute multiple tool calls in parallel.
   * Returns an array of tool-role ChatMessages in the same order.
   */
  async executeToolCalls(
    toolCalls: ToolCall[],
    manifest: AgentManifest,
    agentInstanceId?: string,
    containerId?: string,
  ): Promise<ChatMessage[]> {
    return Promise.all(toolCalls.map((tc) => this.executeTool(tc, manifest, agentInstanceId, containerId)));
  }

  // ── Helpers ───────────────────────────────────────────────────────────────

  /**
   * Convert a SkillInfo into an OpenAI ToolDefinition.
   */
  private static skillToToolDef(skill: SkillInfo): ToolDefinition {
    const properties: Record<string, Record<string, unknown>> = {};
    const required: string[] = [];

    for (const param of skill.parameters) {
      properties[param.name] = {
        type: ToolExecutor.skillTypeToJsonType(param.type),
        description: param.description,
      };
      if (param.required) {
        required.push(param.name);
      }
    }

    const parameters: Record<string, unknown> = {
      type: 'object',
      properties,
    };
    if (required.length > 0) {
      parameters['required'] = required;
    }

    return {
      type: 'function',
      function: {
        name: skill.id,
        description: skill.description,
        parameters,
      },
    };
  }

  /**
   * Map SkillParameter types to JSON Schema types.
   */
  private static skillTypeToJsonType(
    type: SkillParameter['type'],
  ): string {
    switch (type) {
      case 'string': return 'string';
      case 'number': return 'number';
      case 'boolean': return 'boolean';
      case 'array': return 'array';
      case 'object': return 'object';
      default: return 'string';
    }
  }

  /**
   * Truncate a string to MAX_RESULT_LENGTH, appending a marker if truncated.
   */
  private static truncate(content: string): string {
    if (content.length <= MAX_RESULT_LENGTH) return content;
    return content.substring(0, MAX_RESULT_LENGTH) + '\n\n[TRUNCATED — output exceeded 50,000 characters]';
  }

  /**
   * Create a promise that rejects after the given timeout.
   */
  private static timeout(ms: number, skillId: string): Promise<never> {
    return new Promise((_resolve, reject) => {
      setTimeout(
        () => reject(new Error(`Tool "${skillId}" timed out after ${ms / 1000}s`)),
        ms,
      );
    });
  }
}
