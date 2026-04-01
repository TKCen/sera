/**
 * HookRunner — executes pre- and post-tool execution hooks.
 *
 * Hooks are shell commands defined in the agent manifest that can
 * deny execution (PreToolUse), warn, or provide additional feedback.
 */

import { spawnSync } from 'child_process';
import { log } from './logger.js';

export type HookEvent = 'PreToolUse' | 'PostToolUse';

export interface HookRunResult {
  allowed: boolean;
  feedback?: string;
  warning?: string;
}

export class HookRunner {
  private workspacePath: string;

  constructor(workspacePath: string = '/workspace') {
    this.workspacePath = workspacePath;
  }

  /**
   * Execute a set of hooks for a given event.
   */
  async runHooks(
    event: HookEvent,
    hooks: string[],
    toolName: string,
    toolInput: string,
    toolOutput?: string,
    isError: boolean = false
  ): Promise<HookRunResult> {
    let combinedFeedback = '';
    let combinedWarning = '';

    for (const hookCmd of hooks) {
      try {
        const payload = JSON.stringify({
          event,
          toolName,
          toolInput: JSON.parse(toolInput),
          toolOutput: toolOutput ? (this.isJson(toolOutput) ? JSON.parse(toolOutput) : toolOutput) : undefined,
          isError,
        });

        const env = {
          ...process.env,
          HOOK_EVENT: event,
          HOOK_TOOL_NAME: toolName,
          HOOK_TOOL_INPUT: toolInput,
          HOOK_TOOL_OUTPUT: toolOutput || '',
          HOOK_TOOL_IS_ERROR: isError ? '1' : '0',
        };

        const result = spawnSync('bash', ['-c', hookCmd], {
          cwd: this.workspacePath,
          input: payload,
          env,
          encoding: 'utf-8',
          timeout: 30_000,
        });

        const stdout = result.stdout?.trim() || '';
        const stderr = result.stderr?.trim() || '';
        const exitCode = result.status ?? -1;

        if (exitCode === 2) {
          // Deny (only relevant for PreToolUse, but we respect it for both)
          log('info', `Hook ${event} denied by ${hookCmd}: ${stdout || 'No reason provided'}`);
          return {
            allowed: false,
            feedback: stdout || 'Tool execution denied by policy hook.',
          };
        }

        if (exitCode !== 0) {
          // Warning
          log('warn', `Hook ${event} warning from ${hookCmd} (exit ${exitCode}): ${stdout} ${stderr}`);
          if (stdout) {
            combinedWarning += (combinedWarning ? '\n' : '') + stdout;
          }
        } else {
          // Allow / Success
          if (stdout) {
            combinedFeedback += (combinedFeedback ? '\n' : '') + stdout;
          }
        }
      } catch (err) {
        log('error', `Failed to execute hook ${hookCmd}: ${err instanceof Error ? err.message : String(err)}`);
        // We don't crash the loop if a hook fails to spawn
      }
    }

    return {
      allowed: true,
      feedback: combinedFeedback || undefined,
      warning: combinedWarning || undefined,
    };
  }

  private isJson(str: string): boolean {
    try {
      JSON.parse(str);
      return true;
    } catch {
      return false;
    }
  }
}
