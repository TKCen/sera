/**
 * RuntimeToolExecutor — native tool execution inside the agent container.
 *
 * Instead of routing through Core's SkillRegistry/ToolExecutor (which would
 * require `docker exec`), tools execute natively using the container's
 * filesystem and shell. All file operations are scoped to /workspace.
 */

import fs from 'fs';
import path from 'path';
import { execSync } from 'child_process';
import type { ChatMessage, ToolCall, ToolDefinition } from './llmClient.js';
import { log } from './logger.js';
import { parseJson } from './json.js';

/** Max output length for tool results. */
const MAX_RESULT_LENGTH = 50_000;

/** Default timeout for shell commands in ms. */
const SHELL_TIMEOUT_MS = 30_000;

// ── Tool Definitions ─────────────────────────────────────────────────────────

const BUILTIN_TOOLS: ToolDefinition[] = [
  {
    type: 'function',
    function: {
      name: 'file-read',
      description: 'Read the contents of a file from the workspace.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Relative path to the file within the workspace.',
          },
        },
        required: ['path'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'file-write',
      description: 'Write content to a file in the workspace. Creates parent directories if needed.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Relative path to the file within the workspace.',
          },
          content: {
            type: 'string',
            description: 'Content to write to the file.',
          },
        },
        required: ['path', 'content'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'shell-exec',
      description: 'Execute a shell command in the workspace directory. Returns stdout + stderr.',
      parameters: {
        type: 'object',
        properties: {
          command: {
            type: 'string',
            description: 'The shell command to execute.',
          },
        },
        required: ['command'],
      },
    },
  },
];

// ── Executor ─────────────────────────────────────────────────────────────────

export class RuntimeToolExecutor {
  private workspacePath: string;

  constructor(workspacePath: string = '/workspace') {
    this.workspacePath = workspacePath;
  }

  /**
   * Get tool definitions that the LLM can call.
   * Filters based on the manifest's allowed tools list.
   */
  getToolDefinitions(allowedTools?: string[]): ToolDefinition[] {
    if (!allowedTools || allowedTools.length === 0) {
      return BUILTIN_TOOLS;
    }

    return BUILTIN_TOOLS.filter((t) =>
      allowedTools.includes(t.function.name),
    );
  }

  /**
   * Execute a single tool call and return a tool-role ChatMessage.
   */
  executeTool(toolCall: ToolCall): ChatMessage {
    const { id, function: fn } = toolCall;
    const toolName = fn.name;

    try {
      let params: Record<string, unknown>;
      try {
        params = parseJson(fn.arguments || '{}');
      } catch {
        return {
          role: 'tool',
          tool_call_id: id,
          content: `Error: Failed to parse tool arguments as JSON: ${fn.arguments}`,
        };
      }

      let result: string;

      switch (toolName) {
        case 'file-read':
          result = this.fileRead(params.path as string);
          break;
        case 'file-write':
          result = this.fileWrite(params.path as string, params.content as string);
          break;
        case 'shell-exec':
          result = this.shellExec(params.command as string);
          break;
        default:
          result = `Error: Unknown tool "${toolName}"`;
      }

      return {
        role: 'tool',
        tool_call_id: id,
        content: this.truncate(result),
      };
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      log('error', `Tool execution error for "${toolName}":`, err);
      return {
        role: 'tool',
        tool_call_id: id,
        content: `Error: ${errorMsg}`,
      };
    }
  }

  /**
   * Execute multiple tool calls sequentially.
   */
  executeToolCalls(toolCalls: ToolCall[]): ChatMessage[] {
    return toolCalls.map((tc) => this.executeTool(tc));
  }

  // ── Built-in Tool Handlers ─────────────────────────────────────────────────

  private fileRead(filePath: string): string {
    const resolved = this.resolveSafe(filePath);
    if (!fs.existsSync(resolved)) {
      return `Error: File not found: ${filePath}`;
    }
    return fs.readFileSync(resolved, 'utf-8');
  }

  private fileWrite(filePath: string, content: string): string {
    const resolved = this.resolveSafe(filePath);
    const dir = path.dirname(resolved);
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(resolved, content, 'utf-8');
    return `File written: ${filePath} (${content.length} bytes)`;
  }

  private shellExec(command: string): string {
    try {
      const output = execSync(command, {
        cwd: this.workspacePath,
        timeout: SHELL_TIMEOUT_MS,
        encoding: 'utf-8',
        maxBuffer: 1024 * 1024, // 1MB
        stdio: ['pipe', 'pipe', 'pipe'],
      });
      return output;
    } catch (err: any) {
      // execSync throws on non-zero exit — capture the output
      const stdout = err.stdout || '';
      const stderr = err.stderr || '';
      const exitCode = err.status ?? -1;
      return `Exit code: ${exitCode}\nSTDOUT:\n${stdout}\nSTDERR:\n${stderr}`;
    }
  }

  // ── Helpers ────────────────────────────────────────────────────────────────

  /**
   * Resolve a file path safely within the workspace.
   * Prevents path traversal attacks.
   */
  private resolveSafe(filePath: string): string {
    const resolved = path.resolve(this.workspacePath, filePath);

    if (!resolved.startsWith(this.workspacePath)) {
      throw new Error(`Path traversal blocked: "${filePath}" resolves outside workspace`);
    }

    return resolved;
  }

  private truncate(content: string): string {
    if (content.length <= MAX_RESULT_LENGTH) return content;
    return (
      content.substring(0, MAX_RESULT_LENGTH) +
      '\n\n[TRUNCATED — output exceeded 50,000 characters]'
    );
  }
}
