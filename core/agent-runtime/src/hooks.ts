import { spawn } from 'child_process';
import { log } from './logger.js';

export type HookEvent = 'PreToolUse' | 'PostToolUse';

export interface HookContext {
  toolName: string;
  toolInput: string;
  toolOutput?: string;
  isError?: boolean;
}

export interface HookRunResult {
  status: 'allow' | 'deny' | 'warn';
  feedback?: string;
}

export class HookRunner {
  private preHooks: string[];
  private postHooks: string[];

  constructor(hooks?: { preToolUse?: string[]; postToolUse?: string[] }) {
    this.preHooks = hooks?.preToolUse || [];
    this.postHooks = hooks?.postToolUse || [];
  }

  /**
   * Run all hooks for a given event.
   * Hooks run sequentially. The first hook to deny (exit 2) stops further execution.
   * Feedback from all hooks is concatenated.
   */
  async run(event: HookEvent, context: HookContext): Promise<HookRunResult> {
    const commands = event === 'PreToolUse' ? this.preHooks : this.postHooks;
    if (commands.length === 0) {
      return { status: 'allow' };
    }

    let finalStatus: 'allow' | 'deny' | 'warn' = 'allow';
    const feedbacks: string[] = [];

    for (const command of commands) {
      const result = await this.runCommand(command, event, context);

      if (result.feedback) {
        feedbacks.push(result.feedback);
      }

      if (result.status === 'deny') {
        return {
          status: 'deny',
          feedback: feedbacks.join('\n\n'),
        };
      }

      if (result.status === 'warn') {
        finalStatus = 'warn';
      }
    }

    return {
      status: finalStatus,
      feedback: feedbacks.length > 0 ? feedbacks.join('\n\n') : undefined,
    };
  }

  private async runCommand(
    command: string,
    event: HookEvent,
    context: HookContext
  ): Promise<HookRunResult> {
    return new Promise((resolve) => {
      const env = {
        ...process.env,
        HOOK_EVENT: event,
        HOOK_TOOL_NAME: context.toolName,
        HOOK_TOOL_INPUT: context.toolInput,
        HOOK_TOOL_OUTPUT: context.toolOutput || '',
        HOOK_TOOL_IS_ERROR: context.isError ? '1' : '0',
      };

      const payload = JSON.stringify({
        event,
        toolName: context.toolName,
        toolInput: context.toolInput,
        toolOutput: context.toolOutput,
        isError: context.isError,
      });

      log('debug', `Running hook (${event}): ${command}`);

      // We use /bin/sh -c to support complex commands with pipes/args
      const child = spawn('/bin/sh', ['-c', command], {
        env,
        stdio: ['pipe', 'pipe', 'pipe'],
      });

      let stdout = '';
      let stderr = '';

      child.stdout?.on('data', (data) => {
        stdout += data.toString();
      });

      child.stderr?.on('data', (data) => {
        stderr += data.toString();
      });

      child.stdin?.on('error', (err) => {
        log('debug', `Hook stdin error (${command}): ${err.message}`);
      });
      child.stdin?.write(payload);
      child.stdin?.end();

      child.on('error', (err) => {
        log('error', `Hook spawn error (${command}): ${err.message}`);
        resolve({ status: 'warn', feedback: `Hook execution failed: ${err.message}` });
      });

      child.on('exit', (code) => {
        const output = stdout.trim();
        const errorOutput = stderr.trim();

        if (code === 0) {
          resolve({ status: 'allow', feedback: output || undefined });
        } else if (code === 2) {
          log('info', `Hook denied execution (exit 2): ${command}`);
          resolve({ status: 'deny', feedback: output || errorOutput || 'Access denied by hook.' });
        } else {
          log('warn', `Hook warning (exit ${code}): ${command}. Stderr: ${errorOutput}`);
          // For warnings, we might want to include the error output if stdout is empty
          resolve({ status: 'warn', feedback: output || errorOutput || undefined });
        }
      });
    });
  }
}
