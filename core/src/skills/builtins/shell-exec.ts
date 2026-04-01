import { execSync } from 'child_process';
import type { SkillDefinition } from '../types.js';
import { TierPolicy } from '../../sandbox/index.js';

/**
 * Built-in skill: shell-exec
 *
 * Executes a shell command in the agent's container (sandboxed) or
 * falls back to local execution if no container is available.
 */
export const shellExecSkill: SkillDefinition = {
  id: 'shell-exec',
  description: 'Execute a shell command in the workspace directory.',
  source: 'builtin',
  parameters: [
    {
      name: 'command',
      type: 'string',
      description: 'The shell command to execute',
      required: true,
    },
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
      // ── Container Execution (preferred — sandboxed) ─────────────────────
      if (context.containerId && context.sandboxManager) {
        const result = await context.sandboxManager.exec(context.manifest, {
          containerId: context.containerId,
          agentName: context.agentName,
          command: ['bash', '-c', command],
        });

        if (result.exitCode !== 0) {
          return {
            success: false,
            error: `Command failed (exit ${result.exitCode}): ${result.output}`,
          };
        }
        return { success: true, data: result.output };
      }

      // ── Local Execution (fallback — no container) ───────────────────────
      const output = execSync(command, {
        cwd: context.workspacePath,
        timeout: 30000,
        encoding: 'utf-8',
        stdio: 'pipe',
      });

      return { success: true, data: output };
    } catch (err: unknown) {
      const error = err as { message?: string; stderr?: string; stdout?: string };
      let errorMessage = error.message || String(err);

      if (error.stderr) {
        errorMessage += `\nStderr: ${error.stderr}`;
      }
      if (error.stdout) {
        errorMessage += `\nStdout: ${error.stdout}`;
      }

      return {
        success: false,
        error: errorMessage,
      };
    }
  },
};
