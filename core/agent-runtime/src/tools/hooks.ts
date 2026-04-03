/**
 * Tool execution hooks — interfaces and runner for beforeToolCall/afterToolCall.
 */

import { log } from '../logger.js';
import { spawn } from 'child_process';

export type HookEvent = 'before_tool_call' | 'after_tool_call';

export interface HookContext {
  toolName: string;
  args: Record<string, unknown>;
  result?: string;
  isError?: boolean;
  agentName: string;
  agentInstanceId: string;
  tier: number;
}

export interface HookResponse {
  status: 'allow' | 'deny' | 'warn';
  modifiedArgs?: Record<string, unknown>;
  modifiedResult?: string;
  message?: string;
}

export interface HookConfig {
  command: string;
  events: HookEvent[];
}

export class HookRunner {
  constructor(private readonly configs: HookConfig[]) {}

  /**
   * Execute all applicable before_tool_call hooks.
   * If any hook denies, returns status 'deny'.
   * Subsequent hooks receive the modifiedArgs from previous ones.
   */
  async beforeToolCall(context: HookContext): Promise<HookResponse> {
    let currentArgs = { ...context.args };
    let finalStatus: 'allow' | 'warn' = 'allow';
    let finalMessage: string | undefined;

    const applicable = this.configs.filter((c) => c.events.includes('before_tool_call'));

    for (const config of applicable) {
      try {
        const result = await this.executeHook(
          config,
          { ...context, args: currentArgs },
          'before_tool_call'
        );

        if (result.status === 'deny') {
          return result;
        }

        if (result.status === 'warn') {
          finalStatus = 'warn';
          finalMessage = result.message;
        }

        if (result.modifiedArgs) {
          currentArgs = result.modifiedArgs;
        }
      } catch (err) {
        log(
          'error',
          `Hook error (beforeToolCall): ${err instanceof Error ? err.message : String(err)}`
        );
        // Hook errors don't crash the reasoning loop (logged and skipped)
      }
    }

    return { status: finalStatus, modifiedArgs: currentArgs, message: finalMessage };
  }

  /**
   * Execute all applicable after_tool_call hooks.
   * Subsequent hooks receive the modifiedResult from previous ones.
   */
  async afterToolCall(context: HookContext): Promise<HookResponse> {
    let currentResult = context.result;
    let finalStatus: 'allow' | 'warn' = 'allow';
    let finalMessage: string | undefined;

    const applicable = this.configs.filter((c) => c.events.includes('after_tool_call'));

    for (const config of applicable) {
      try {
        const result = await this.executeHook(
          config,
          { ...context, result: currentResult },
          'after_tool_call'
        );

        if (result.status === 'warn') {
          finalStatus = 'warn';
          finalMessage = result.message;
        }

        if (result.modifiedResult !== undefined) {
          currentResult = result.modifiedResult;
        }
      } catch (err) {
        log(
          'error',
          `Hook error (afterToolCall): ${err instanceof Error ? err.message : String(err)}`
        );
      }
    }

    return { status: finalStatus, modifiedResult: currentResult, message: finalMessage };
  }

  private async executeHook(
    config: HookConfig,
    context: HookContext,
    event: HookEvent
  ): Promise<HookResponse> {
    return new Promise((resolve, reject) => {
      const env = {
        ...process.env,
        HOOK_EVENT: event,
        HOOK_TOOL_NAME: context.toolName,
        HOOK_TOOL_INPUT: JSON.stringify(context.args),
        HOOK_TOOL_OUTPUT: context.result || '',
        HOOK_TOOL_IS_ERROR: String(!!context.isError),
      };

      const child = spawn(config.command, [], {
        shell: true,
        env,
        timeout: 30000, // Mandatory 30-second execution timeout
      });

      let stdout = '';
      let stderr = '';

      child.stdout.on('data', (data) => {
        stdout += data.toString();
      });

      child.stderr.on('data', (data) => {
        stderr += data.toString();
      });

      child.on('error', reject);

      child.stdin.on('error', (err: any) => {
        if (err.code === 'EPIPE') {
          // Process exited before we could finish writing - this is fine if it succeeded or we handle the exit code
          log('debug', `Hook ${config.command} closed stdin early (EPIPE)`);
          return;
        }
        reject(err);
      });

      child.on('close', (code) => {
        if (code === 2) {
          resolve({ status: 'deny', message: stderr.trim() || 'Tool execution denied by hook' });
          return;
        }

        const status = code === 0 ? 'allow' : 'warn';
        const message = stderr.trim();

        if (status === 'allow' && stdout.trim()) {
          try {
            const parsed = JSON.parse(stdout);
            if (event === 'before_tool_call') {
              resolve({ status, modifiedArgs: parsed, message });
            } else {
              resolve({
                status,
                modifiedResult: typeof parsed === 'string' ? parsed : JSON.stringify(parsed),
                message,
              });
            }
            return;
          } catch {
            // If not JSON, use as raw string for result, or ignore for args
            if (event === 'after_tool_call') {
              resolve({ status, modifiedResult: stdout.trim(), message });
            } else {
              resolve({ status, message });
            }
            return;
          }
        }

        resolve({ status, message });
      });

      child.stdin.write(JSON.stringify(context));
      child.stdin.end();
    });
  }
}
