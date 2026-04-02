/**
 * Shell execution handler — tier-gated bash command execution.
 */

import { spawnSync, spawn } from 'child_process';
import { StringDecoder } from 'string_decoder';
import { NotPermittedError, DEFAULT_SHELL_TIMEOUT_MS } from './types.js';
import type { ToolOutputCallback } from '../centrifugo.js';

/**
 * Execute a shell command in the workspace directory.
 * Tier 1 agents cannot use shell-exec.
 */
export function shellExec(
  workspacePath: string,
  tier: number,
  command: string,
  timeoutMs?: number
): string {
  if (tier === 1) {
    throw new NotPermittedError('shell-exec is not available for tier-1 agents');
  }

  const timeout = timeoutMs ?? DEFAULT_SHELL_TIMEOUT_MS;

  const result = spawnSync('bash', ['-c', command], {
    cwd: workspacePath,
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

/**
 * Execute a shell command with streaming stdout/stderr via onOutput callback.
 * Uses spawn() for live output instead of spawnSync().
 * Falls back to shellExec() if spawn fails to start.
 */
export async function shellExecStreaming(
  workspacePath: string,
  tier: number,
  command: string,
  timeoutMs: number | undefined,
  onOutput: ToolOutputCallback,
  toolCallId: string
): Promise<string> {
  if (tier === 1) {
    throw new NotPermittedError('shell-exec is not available for tier-1 agents');
  }

  const timeout = timeoutMs ?? DEFAULT_SHELL_TIMEOUT_MS;
  const start = Date.now();

  return new Promise<string>((resolve) => {
    let child: ReturnType<typeof spawn>;
    try {
      child = spawn('bash', ['-c', command], {
        cwd: workspacePath,
        shell: false,
      });
    } catch (err) {
      // Spawn failed to start — fall back to sync variant
      resolve(shellExec(workspacePath, tier, command, timeoutMs));
      return;
    }

    const stdoutChunks: string[] = [];
    const stderrChunks: string[] = [];
    let stdoutBytes = 0;
    let stderrBytes = 0;
    const MAX_STREAM_BYTES = 2 * 1024 * 1024; // 2 MB, same as sync variant
    let timedOut = false;

    const timer = setTimeout(() => {
      timedOut = true;
      child.kill('SIGKILL');
    }, timeout);

    const emitLines = (buffer: string, type: 'stdout' | 'stderr', pending: string[]): string => {
      const parts = (pending.join('') + buffer).split('\n');
      // Last element may be incomplete — keep it as pending
      const incomplete = parts.pop() ?? '';
      for (const line of parts) {
        onOutput({
          toolCallId,
          toolName: 'shell-exec',
          type,
          content: line,
          done: false,
          timestamp: new Date().toISOString(),
        });
      }
      pending.length = 0;
      if (incomplete) pending.push(incomplete);
      return incomplete;
    };

    const stdoutPending: string[] = [];
    const stderrPending: string[] = [];
    const stdoutDecoder = new StringDecoder('utf-8');
    const stderrDecoder = new StringDecoder('utf-8');

    child.stdout?.on('data', (chunk: Buffer) => {
      const text = stdoutDecoder.write(chunk);
      stdoutBytes += chunk.length;
      if (stdoutBytes <= MAX_STREAM_BYTES) {
        stdoutChunks.push(text);
      } else if (!timedOut) {
        child.kill('SIGKILL');
      }
      emitLines(text, 'stdout', stdoutPending);
    });

    child.stderr?.on('data', (chunk: Buffer) => {
      const text = stderrDecoder.write(chunk);
      stderrBytes += chunk.length;
      if (stderrBytes <= MAX_STREAM_BYTES) {
        stderrChunks.push(text);
      } else if (!timedOut) {
        child.kill('SIGKILL');
      }
      emitLines(text, 'stderr', stderrPending);
    });

    child.on('error', (err) => {
      clearTimeout(timer);
      onOutput({
        toolCallId,
        toolName: 'shell-exec',
        type: 'error',
        content: err.message,
        done: true,
        timestamp: new Date().toISOString(),
        durationMs: Date.now() - start,
      });
      resolve(shellExec(workspacePath, tier, command, timeoutMs));
    });

    child.on('close', (code) => {
      clearTimeout(timer);

      const finalStdout = stdoutDecoder.end();
      if (finalStdout) {
        stdoutChunks.push(finalStdout);
        emitLines(finalStdout, 'stdout', stdoutPending);
      }

      const finalStderr = stderrDecoder.end();
      if (finalStderr) {
        stderrChunks.push(finalStderr);
        emitLines(finalStderr, 'stderr', stderrPending);
      }

      // Flush any remaining partial lines
      for (const pending of [stdoutPending, stderrPending]) {
        const remainder = pending.join('');
        if (remainder) {
          const type = pending === stdoutPending ? 'stdout' : 'stderr';
          onOutput({
            toolCallId,
            toolName: 'shell-exec',
            type,
            content: remainder,
            done: false,
            timestamp: new Date().toISOString(),
          });
        }
      }

      const stdout = stdoutChunks.join('');
      const stderr = stderrChunks.join('');
      const exitCode = timedOut ? -1 : (code ?? -1);
      const durationMs = Date.now() - start;

      let resultStr: string;
      if (exitCode === 0) {
        resultStr = stdout;
      } else {
        resultStr = `Exit code: ${exitCode}\nSTDOUT:\n${stdout}\nSTDERR:\n${stderr}`;
      }

      onOutput({
        toolCallId,
        toolName: 'shell-exec',
        result: resultStr.substring(0, 500),
        duration: durationMs,
        error: exitCode !== 0,
        timestamp: new Date().toISOString(),
      });

      resolve(resultStr);
    });
  });
}

/**
 * Check if a shell command references a path outside /workspace.
 */
export function checkShellPathRestriction(
  workspacePath: string,
  command: string
): string | undefined {
  const absPathPattern = /(?:^|\s)(\/(?!workspace\b)[^\s]+)/g;
  let match: RegExpExecArray | null;
  while ((match = absPathPattern.exec(command)) !== null) {
    const matchedPath = match[1];
    if (matchedPath && !matchedPath.startsWith(workspacePath)) {
      return matchedPath;
    }
  }
  return undefined;
}
