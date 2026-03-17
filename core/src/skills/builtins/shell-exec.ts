import { execSync } from 'child_process';
import type { SkillDefinition } from '../types.js';
import { TierPolicy } from '../../sandbox/TierPolicy.js';

/**
 * Built-in skill: shell-exec
 *
 * Executes a shell command in the workspace directory.
 */
export const shellExecSkill: SkillDefinition = {
  id: 'shell-exec',
  description: 'Execute a shell command in the workspace directory.',
  source: 'builtin',
  parameters: [
    { name: 'command', type: 'string', description: 'The shell command to execute', required: true },
  ],
  handler: async (params, context) => {
    if (!TierPolicy.canExec(context.manifest)) {
      return { success: false, error: 'Agent is not permitted to execute shell commands' };
    }

    const command = params['command'];
    if (!command || typeof command !== 'string') {
      return { success: false, error: 'Parameter "command" is required and must be a string' };
    }

    try {
      const output = execSync(command, {
        cwd: context.workspacePath,
        timeout: 30000,
        encoding: 'utf-8',
        stdio: 'pipe',
      });

      return { success: true, data: output };
    } catch (err: any) {
      let errorMessage = err instanceof Error ? err.message : String(err);

      if (err.stderr) {
        errorMessage += `\nStderr: ${err.stderr}`;
      }
      if (err.stdout) {
        errorMessage += `\nStdout: ${err.stdout}`;
      }

      return {
        success: false,
        error: errorMessage,
      };
    }
  },
};
