/**
 * RuntimeToolExecutor — native tool execution inside the agent container.
 *
 * Tools execute natively using the container's filesystem and shell.
 * All file operations are scoped to /workspace.
 */

import fs from 'fs';
import path from 'path';
import { spawnSync } from 'child_process';
import type { ChatMessage, ToolCall, ToolDefinition } from './llmClient.js';
import { log } from './logger.js';
import { parseJson } from './json.js';

// ── Error Types ───────────────────────────────────────────────────────────────

export class PermissionDeniedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'PermissionDeniedError';
  }
}

export class NotPermittedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'NotPermittedError';
  }
}

// ── Constants ─────────────────────────────────────────────────────────────────

/** Max output length in bytes (50 KB). */
const MAX_RESULT_BYTES = 50_000;

/** Default timeout for shell commands in ms. */
const DEFAULT_SHELL_TIMEOUT_MS = 30_000;

const AGENT_ID = process.env['AGENT_INSTANCE_ID'] || process.env['AGENT_NAME'] || 'unknown';

// ── Tool Definitions ──────────────────────────────────────────────────────────

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
      name: 'file-list',
      description: 'List directory contents with type (file/dir) and size.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Relative path to the directory within the workspace. Defaults to root.',
          },
        },
        required: [],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'file-delete',
      description: 'Delete a file or empty directory in the workspace.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Relative path to the file or directory within the workspace.',
          },
          recursive: {
            type: 'boolean',
            description: 'If true, delete a non-empty directory and all its contents.',
          },
        },
        required: ['path'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'shell-exec',
      description: 'Execute a shell command in the workspace directory. Returns stdout/stderr/exitCode.',
      parameters: {
        type: 'object',
        properties: {
          command: {
            type: 'string',
            description: 'The shell command to execute.',
          },
          timeout_ms: {
            type: 'number',
            description: 'Command timeout in milliseconds. Defaults to 30000.',
          },
        },
        required: ['command'],
      },
    },
  },
];

// ── Executor ──────────────────────────────────────────────────────────────────

export class RuntimeToolExecutor {
  private workspacePath: string;
  /** Tier 1 agents cannot execute shell commands. */
  private tier: number;

  constructor(workspacePath: string = '/workspace', tier?: number) {
    this.workspacePath = workspacePath;
    // DECISION: tier read from AGENT_TIER env var if not passed explicitly;
    // defaults to 2 (standard) if unset.
    this.tier = tier ?? (process.env['AGENT_TIER'] ? parseInt(process.env['AGENT_TIER'], 10) : 2);
  }

  /**
   * Get tool definitions that the LLM can call.
   * Filters based on the manifest's allowed tools list.
   */
  getToolDefinitions(allowedTools?: string[]): ToolDefinition[] {
    if (!allowedTools || allowedTools.length === 0) {
      return BUILTIN_TOOLS;
    }
    return BUILTIN_TOOLS.filter((t) => allowedTools.includes(t.function.name));
  }

  /**
   * Execute a single tool call and return a tool-role ChatMessage.
   */
  executeTool(toolCall: ToolCall): ChatMessage {
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
          result = this.fileRead(params['path'] as string);
          break;
        case 'file-write':
          result = this.fileWrite(params['path'] as string, params['content'] as string);
          break;
        case 'file-list':
          result = this.fileList(params['path'] as string | undefined);
          break;
        case 'file-delete':
          result = this.fileDelete(params['path'] as string, params['recursive'] as boolean | undefined);
          break;
        case 'shell-exec':
          result = this.shellExec(
            params['command'] as string,
            params['timeout_ms'] as number | undefined,
          );
          break;
        default:
          result = `Error: Unknown tool "${toolName}"`;
      }

      this.logInvocation(toolName, 'success', Date.now() - start);
      return { role: 'tool', tool_call_id: id, content: this.truncate(result) };
    } catch (err) {
      // Story 3.10: If a file tool path is outside workspace, try proxying
      if (err instanceof PermissionDeniedError && this.isProxyAvailable()) {
        const fileTools = new Set(['file-read', 'file-write', 'file-list', 'file-delete']);
        if (fileTools.has(toolName)) {
          // Parse args again (already parsed above but out of scope)
          const params = parseJson(fn.arguments || '{}');
          return this.executeProxiedTool(id, toolName, params, start);
        }
      }

      const errorMsg = err instanceof Error ? err.message : String(err);
      this.logInvocation(toolName, 'error', Date.now() - start);
      return { role: 'tool', tool_call_id: id, content: `Error: ${errorMsg}` };
    }
  }

  /**
   * Execute multiple tool calls sequentially.
   */
  executeToolCalls(toolCalls: ToolCall[]): ChatMessage[] {
    return toolCalls.map((tc) => this.executeTool(tc));
  }

  // ── Built-in Tool Handlers ────────────────────────────────────────────────

  private fileRead(filePath: string): string {
    const resolved = this.resolveSafe(filePath);
    if (!fs.existsSync(resolved)) {
      return `Error: File not found: ${filePath}`;
    }

    const stat = fs.statSync(resolved);

    // Binary files: return base64 with MIME hint
    if (this.isBinaryFile(resolved)) {
      const buf = fs.readFileSync(resolved);
      const mime = this.guessMime(resolved);
      return `[binary:${mime}]\n${buf.toString('base64')}`;
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

  private fileList(dirPath?: string): string {
    const resolved = this.resolveSafe(dirPath || '.');

    if (!fs.existsSync(resolved)) {
      return `Error: Directory not found: ${dirPath ?? '.'}`;
    }

    const stat = fs.statSync(resolved);
    if (!stat.isDirectory()) {
      return `Error: Not a directory: ${dirPath ?? '.'}`;
    }

    const entries = fs.readdirSync(resolved, { withFileTypes: true });
    if (entries.length === 0) {
      return '(empty directory)';
    }

    const lines = entries.map((e) => {
      const type = e.isDirectory() ? 'dir' : 'file';
      let size = '-';
      if (e.isFile()) {
        try {
          const s = fs.statSync(path.join(resolved, e.name));
          size = `${s.size}`;
        } catch {
          // ignore stat errors
        }
      }
      return `${type}\t${size}\t${e.name}`;
    });

    return `type\tsize\tname\n${lines.join('\n')}`;
  }

  private fileDelete(filePath: string, recursive?: boolean): string {
    const resolved = this.resolveSafe(filePath);

    if (!fs.existsSync(resolved)) {
      return `Error: File not found: ${filePath}`;
    }

    const stat = fs.statSync(resolved);

    if (stat.isDirectory()) {
      const entries = fs.readdirSync(resolved);
      if (entries.length > 0 && !recursive) {
        return `Error: Directory not empty: ${filePath} (use recursive: true to delete non-empty directories)`;
      }
      fs.rmSync(resolved, { recursive: true, force: true });
      return `Deleted directory: ${filePath}`;
    }

    fs.unlinkSync(resolved);
    return `Deleted file: ${filePath}`;
  }

  private shellExec(command: string, timeoutMs?: number): string {
    if (this.tier === 1) {
      throw new NotPermittedError('shell-exec is not available for tier-1 agents');
    }

    // Story 3.10: Check if shell command references paths outside workspace
    // that would require a persistent grant + container restart
    const outsidePath = this.checkShellPathRestriction(command);
    if (outsidePath && this.isProxyAvailable()) {
      return JSON.stringify({
        error: 'path_requires_restart',
        hint: `Path "${outsidePath}" is outside /workspace. Shell access to dynamically granted paths requires a persistent grant and container restart. Use POST /api/agents/:id/restart after granting persistent access.`,
      });
    }

    const timeout = timeoutMs ?? DEFAULT_SHELL_TIMEOUT_MS;

    const result = spawnSync('bash', ['-c', command], {
      cwd: this.workspacePath,
      timeout,
      encoding: 'utf-8',
      maxBuffer: 2 * 1024 * 1024,
    });

    const stdout = result.stdout ?? '';
    const stderr = result.stderr ?? '';
    const exitCode = result.status ?? -1;

    if (exitCode === 0) {
      return stdout;
    }

    return `Exit code: ${exitCode}\nSTDOUT:\n${stdout}\nSTDERR:\n${stderr}`;
  }

  // ── Proxy Support (Story 3.10) ────────────────────────────────────────────

  /** Check if the sera-core proxy is available. Read env at call time for testability. */
  private isProxyAvailable(): boolean {
    return !!(process.env['SERA_CORE_URL'] && process.env['SERA_IDENTITY_TOKEN']);
  }

  /**
   * Forward a file tool call to sera-core's /v1/tools/proxy endpoint.
   * The proxy validates that the agent has an active grant for the path.
   */
  private executeProxiedTool(
    toolCallId: string,
    toolName: string,
    args: Record<string, unknown>,
    startMs: number,
  ): ChatMessage {
    try {
      // Use synchronous XMLHttpRequest pattern via spawnSync + curl for bun compat
      // Bun supports top-level await but executeTool is sync — use spawnSync
      const coreUrl = process.env['SERA_CORE_URL'] ?? '';
      const token = process.env['SERA_IDENTITY_TOKEN'] ?? '';
      const body = JSON.stringify({ tool: toolName, args });
      const result = spawnSync('curl', [
        '-s', '-X', 'POST',
        `${coreUrl}/v1/tools/proxy`,
        '-H', 'Content-Type: application/json',
        '-H', `Authorization: Bearer ${token}`,
        '-d', body,
        '-w', '\n%{http_code}',
      ], {
        timeout: 30_000,
        encoding: 'utf-8',
        maxBuffer: 2 * 1024 * 1024,
      });

      const output = result.stdout ?? '';
      const lines = output.trimEnd().split('\n');
      const httpStatus = lines.pop() ?? '';
      const responseBody = lines.join('\n');

      if (httpStatus === '403') {
        this.logInvocation(toolName, 'error', Date.now() - startMs);
        return {
          role: 'tool',
          tool_call_id: toolCallId,
          content: `Error: No active grant for this path. Request filesystem access first.`,
        };
      }

      if (httpStatus !== '200') {
        this.logInvocation(toolName, 'error', Date.now() - startMs);
        return {
          role: 'tool',
          tool_call_id: toolCallId,
          content: `Error: Proxy returned HTTP ${httpStatus}: ${responseBody}`,
        };
      }

      const parsed = parseJson(responseBody);
      const content = parsed['result'] as string | undefined ?? parsed['error'] as string | undefined ?? responseBody;
      this.logInvocation(toolName, 'success', Date.now() - startMs);
      return { role: 'tool', tool_call_id: toolCallId, content: this.truncate(String(content)) };
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      this.logInvocation(toolName, 'error', Date.now() - startMs);
      return { role: 'tool', tool_call_id: toolCallId, content: `Error: Proxy call failed: ${errorMsg}` };
    }
  }

  /**
   * Check if a shell command references a path that is outside /workspace
   * and would require a persistent grant + restart for shell access.
   */
  private checkShellPathRestriction(command: string): string | undefined {
    // Simple heuristic: check if the command contains absolute paths outside workspace
    const absPathPattern = /(?:^|\s)(\/(?!workspace\b)[^\s]+)/g;
    let match: RegExpExecArray | null;
    while ((match = absPathPattern.exec(command)) !== null) {
      const matchedPath = match[1];
      if (matchedPath && !matchedPath.startsWith(this.workspacePath)) {
        return matchedPath;
      }
    }
    return undefined;
  }

  // ── Helpers ───────────────────────────────────────────────────────────────

  /**
   * Resolve a path safely within the workspace.
   * @throws PermissionDeniedError if path resolves outside workspace.
   */
  private resolveSafe(filePath: string): string {
    const resolved = path.resolve(this.workspacePath, filePath);

    if (!resolved.startsWith(this.workspacePath + path.sep) && resolved !== this.workspacePath) {
      throw new PermissionDeniedError(
        `Path traversal blocked: "${filePath}" resolves outside workspace`,
      );
    }

    return resolved;
  }

  private truncate(content: string): string {
    if (Buffer.byteLength(content, 'utf-8') <= MAX_RESULT_BYTES) return content;
    // Truncate to byte boundary
    const buf = Buffer.from(content, 'utf-8').slice(0, MAX_RESULT_BYTES);
    return buf.toString('utf-8') + '\n\n[TRUNCATED — output exceeded 50 KB]';
  }

  private isBinaryFile(filePath: string): boolean {
    const ext = path.extname(filePath).toLowerCase();
    const binaryExts = new Set([
      '.png', '.jpg', '.jpeg', '.gif', '.bmp', '.webp', '.ico',
      '.pdf', '.zip', '.tar', '.gz', '.bz2', '.7z', '.rar',
      '.exe', '.dll', '.so', '.dylib', '.wasm',
      '.mp3', '.mp4', '.wav', '.ogg', '.avi', '.mov',
      '.ttf', '.otf', '.woff', '.woff2',
    ]);
    if (binaryExts.has(ext)) return true;

    // Sample first 512 bytes for null bytes
    try {
      const fd = fs.openSync(filePath, 'r');
      const buf = Buffer.alloc(512);
      const bytesRead = fs.readSync(fd, buf, 0, 512, 0);
      fs.closeSync(fd);
      for (let i = 0; i < bytesRead; i++) {
        if (buf[i] === 0) return true;
      }
    } catch {
      // If we can't read it, treat as text
    }

    return false;
  }

  private guessMime(filePath: string): string {
    const ext = path.extname(filePath).toLowerCase();
    const mimes: Record<string, string> = {
      '.png': 'image/png',
      '.jpg': 'image/jpeg',
      '.jpeg': 'image/jpeg',
      '.gif': 'image/gif',
      '.pdf': 'application/pdf',
      '.zip': 'application/zip',
    };
    return mimes[ext] ?? 'application/octet-stream';
  }

  private logInvocation(toolName: string, status: 'success' | 'error', elapsedMs: number): void {
    log('debug', `tool=${toolName} agent=${AGENT_ID} status=${status} elapsed=${elapsedMs}ms`);
  }
}
