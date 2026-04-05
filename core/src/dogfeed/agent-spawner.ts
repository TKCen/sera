/**
 * AgentSpawner — routes tasks to the appropriate coding agent and spawns it in Docker.
 *
 * Both tiers run in Docker containers with a fresh clone of the repo:
 *   - Trivial tasks → pi-agent + local Qwen 3.5 (free, network_mode: host for LM Studio)
 *   - Complex tasks → OMC (subscription, with ~/.claude/ bind-mounted)
 *
 * Container flow:
 *   1. Clone repo from mounted .git directory
 *   2. Checkout the dogfeed branch
 *   3. Agent edits files
 *   4. Commit and push changes
 */

import Docker from 'dockerode';
import { Logger } from '../lib/logger.js';
import type { DogfeedTask, AgentTier, DogfeedConfig } from './types.js';
import { TRIVIAL_CATEGORIES } from './types.js';
import { OMC_DOCKER_IMAGE, MAX_AGENT_OUTPUT_LENGTH } from './constants.js';

const logger = new Logger('AgentSpawner');

export interface AgentResult {
  success: boolean;
  output: string;
  exitCode: number;
  durationMs: number;
  timedOut: boolean;
}

export class AgentSpawner {
  private docker: Docker;

  constructor(
    private readonly config: DogfeedConfig,
    docker?: Docker
  ) {
    this.docker =
      docker ??
      new Docker(
        process.platform === 'win32'
          ? { socketPath: '//./pipe/docker_engine' }
          : { socketPath: '/var/run/docker.sock' }
      );
  }

  /**
   * Route a task to the appropriate agent tier based on its category.
   */
  routeTask(task: DogfeedTask): AgentTier {
    if (TRIVIAL_CATEGORIES.has(task.category)) {
      return 'pi-agent';
    }
    return 'omc';
  }

  /**
   * Spawn a Docker container for the task.
   * The container clones the repo, checks out the branch, runs the agent, commits, and pushes.
   */
  async spawn(task: DogfeedTask, branchName: string): Promise<AgentResult> {
    const tier = this.routeTask(task);
    logger.info(`Routing task to ${tier}: ${task.description}`);
    return this.spawnContainer(task, branchName, tier);
  }

  /**
   * Spawn a Docker container with either pi-agent or OMC.
   */
  private async spawnContainer(
    task: DogfeedTask,
    branchName: string,
    tier: AgentTier
  ): Promise<AgentResult> {
    const prompt = this.buildPrompt(task);
    const startTime = Date.now();
    const containerName = `sera-dogfeed-${tier}-${Date.now()}`;

    logger.info(`Spawning ${tier} container: ${containerName}`);

    // Clean up stale container
    await this.removeContainerIfExists(containerName);

    const homeDir = process.env.USERPROFILE ?? process.env.HOME ?? '/root';
    const repoRoot = this.config.repoRoot.replace(/\\/g, '/');

    // Build the shell script that runs inside the container
    const script = this.buildContainerScript(task, branchName, tier, prompt);

    // Common binds: mount the repo for cloning and git config for push
    const binds = [`${repoRoot}:/repo:ro`, `${homeDir}/.gitconfig:/root/.gitconfig:ro`];

    // Tier-specific binds
    if (tier === 'omc') {
      binds.push(`${homeDir}/.claude:/root/.claude:ro`);
      binds.push(`${homeDir}/.config/gh:/root/.config/gh:ro`);
    }

    const container = await this.docker.createContainer({
      Image: OMC_DOCKER_IMAGE,
      name: containerName,
      Cmd: ['bash', '-c', script],
      Env: [
        `DOGFEED_TASK=${prompt}`,
        `DOGFEED_BRANCH=${branchName}`,
        `DOGFEED_AGENT=${tier}`,
        `PI_AGENT_MODEL=${this.config.piAgentModel}`,
        `PI_AGENT_PROVIDER=${this.config.piAgentProvider}`,
      ],
      HostConfig: {
        Binds: binds,
        // host network so pi-agent can reach LM Studio on localhost:1234
        NetworkMode: 'host',
      },
      Labels: {
        'sera.dogfeed': 'true',
      },
    });

    await container.start();

    // Wait with timeout
    let timedOut = false;
    const timeoutHandle = setTimeout(async () => {
      timedOut = true;
      try {
        await container.stop({ t: 5 });
      } catch {
        // Already stopped
      }
    }, this.config.agentTimeoutMs);

    try {
      await container.wait();
    } finally {
      clearTimeout(timeoutHandle);
    }

    // Collect output
    const logStream = await container.logs({ stdout: true, stderr: true });
    const output = logStream.toString('utf-8');

    // Get exit code
    const inspectInfo = await container.inspect();
    const exitCode = inspectInfo.State.ExitCode ?? 1;

    // Cleanup
    await container.remove({ force: true }).catch(() => {});

    return {
      success: exitCode === 0 && !timedOut,
      output: truncate(output, MAX_AGENT_OUTPUT_LENGTH),
      exitCode,
      durationMs: Date.now() - startTime,
      timedOut,
    };
  }

  /**
   * Build the shell script that runs inside the container.
   * Clones the repo, checks out the branch, runs the agent, commits, and pushes.
   */
  private buildContainerScript(
    task: DogfeedTask,
    branchName: string,
    tier: AgentTier,
    prompt: string
  ): string {
    // Escape single quotes in prompt for shell embedding
    const escapedPrompt = prompt.replace(/'/g, "'\\''");

    const lines = [
      'set -e',
      '',
      '# Clone from the mounted repo (local clone is fast — hardlinks objects)',
      'git clone /repo /workspace',
      'cd /workspace',
      `git checkout ${branchName}`,
      '',
      '# Configure git for commits',
      `git config user.name "${this.config.gitUserName}"`,
      `git config user.email "${this.config.gitUserEmail}"`,
      '',
      '# Run the coding agent',
      `echo '${escapedPrompt}' | \\`,
    ];

    if (tier === 'pi-agent') {
      lines.push(
        `  pi --model "$PI_AGENT_MODEL" --provider "$PI_AGENT_PROVIDER" --print --no-session`
      );
    } else {
      lines.push(`  claude --print --dangerously-skip-permissions`);
    }

    lines.push(
      '',
      '# Stage and commit any changes',
      'if [ -n "$(git status --porcelain)" ]; then',
      '  git add -A',
      `  git commit -m "dogfeed(${task.category}): ${task.description.replace(/"/g, '\\"')}"`,
      '  # Push back to the mounted repo',
      `  git push origin ${branchName}`,
      '  echo "DOGFEED_CHANGES=yes"',
      'else',
      '  echo "DOGFEED_CHANGES=no"',
      '  exit 1',
      'fi'
    );

    return lines.join('\n');
  }

  private buildPrompt(task: DogfeedTask): string {
    const parts = [
      'You are a coding agent. Edit files directly using your edit/write tools.',
      `Your task: ${task.description}`,
      '',
      'Instructions:',
      '- Use your edit tool to modify the file(s) directly',
      '- Make the MINIMAL change that satisfies the task',
      '- Do not modify unrelated files',
      '- Do not just describe what to do — actually edit the file',
    ];

    if (task.filePath) {
      parts.push(`- Focus on file: ${task.filePath}${task.line ? `:${task.line}` : ''}`);
    }

    return parts.join('\n');
  }

  private async removeContainerIfExists(name: string): Promise<void> {
    try {
      const existing = this.docker.getContainer(name);
      const info = await existing.inspect();
      if (info.State) {
        if (info.State.Running) await existing.stop().catch(() => {});
        await existing.remove({ force: true }).catch(() => {});
      }
    } catch {
      // Expected — container doesn't exist
    }
  }
}

function truncate(str: string, maxLen: number): string {
  if (str.length <= maxLen) return str;
  return str.substring(0, maxLen) + '\n... [truncated]';
}
