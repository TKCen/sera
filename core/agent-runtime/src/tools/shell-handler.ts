/**
 * Shell execution handler — tier-gated bash command execution.
 */

import { spawnSync } from 'child_process';
import { NotPermittedError, DEFAULT_SHELL_TIMEOUT_MS } from './types.js';

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
