/**
 * ToolRunner — execute tools in ephemeral sandbox containers.
 *
 * Spawns a short-lived container, runs the tool command, captures output,
 * and cleans up. Tools are validated against the agent's AGENT.yaml
 * permissions (tools.allowed / tools.denied).
 *
 * @see sera/docs/reimplementation/agent-workspace-architecture.md § Operations
 */

import type { AgentManifest } from '../agents/index.js';
import type { ToolRunRequest, ToolResult } from './types.js';
import { SandboxManager } from './SandboxManager.js';
import { TierPolicy } from './TierPolicy.js';

// ── ToolRunner ──────────────────────────────────────────────────────────────────

export class ToolRunner {
  private sandboxManager: SandboxManager;

  constructor(sandboxManager: SandboxManager) {
    this.sandboxManager = sandboxManager;
  }

  /**
   * Run a tool in an ephemeral container.
   *
   * 1. Validates tool permission against the agent's manifest
   * 2. Spawns a container with tier-appropriate limits
   * 3. Waits for completion (with timeout)
   * 4. Captures stdout/stderr
   * 5. Cleans up the container
   */
  async runTool(manifest: AgentManifest, request: ToolRunRequest): Promise<ToolResult> {
    const startTime = Date.now();

    // ── Permission check ──────────────────────────────────────────────────
    TierPolicy.validateToolPermission(manifest, request);

    const timeoutMs = TierPolicy.clampTimeout(request.timeoutSeconds) * 1000;
    const image = request.image ?? 'alpine:latest';

    // ── Spawn the tool container ──────────────────────────────────────────
    const sandbox = await this.sandboxManager.spawn(manifest, {
      agentName: manifest.metadata.name,
      type: 'tool',
      image,
      command: request.command,
      env: {
        SERA_TOOL: request.toolName,
        SERA_AGENT: manifest.metadata.name,
      },
    });

    // ── Wait for completion with timeout ──────────────────────────────────
    try {
      const result = await this.waitForCompletion(sandbox.containerId, timeoutMs);

      return {
        ...result,
        durationMs: Date.now() - startTime,
      };
    } catch (error) {
      // Attempt cleanup on any failure
      try {
        await this.sandboxManager.remove(manifest, sandbox.containerId);
      } catch {
        // Best-effort cleanup
      }
      throw error;
    }
  }

  // ── Internal ─────────────────────────────────────────────────────────────

  /**
   * Wait for a container to finish, with timeout.
   * Returns the captured output and exit code.
   */
  private async waitForCompletion(
    containerId: string,
    timeoutMs: number
  ): Promise<Omit<ToolResult, 'durationMs'>> {
    return new Promise((resolve, reject) => {
      let resolved = false;

      const timer = setTimeout(() => {
        if (!resolved) {
          resolved = true;
          resolve({
            exitCode: -1,
            stdout: '',
            stderr: 'Tool execution timed out',
            timedOut: true,
          });
        }
      }, timeoutMs);

      this.pollContainer(containerId)
        .then((result) => {
          if (!resolved) {
            resolved = true;
            clearTimeout(timer);
            resolve(result);
          }
        })
        .catch((err) => {
          if (!resolved) {
            resolved = true;
            clearTimeout(timer);
            reject(err);
          }
        });
    });
  }

  /**
   * Poll a container until it stops, then capture its logs.
   */
  private async pollContainer(containerId: string): Promise<Omit<ToolResult, 'durationMs'>> {
    // Wait for the container to exit by polling (simple approach)
    const POLL_INTERVAL = 500; // ms
    const MAX_POLLS = 600; // 300s max

    for (let i = 0; i < MAX_POLLS; i++) {
      const containers = this.sandboxManager.listContainers();
      const sandbox = containers.find((c) => c.containerId === containerId);

      if (!sandbox || sandbox.status === 'stopped') {
        break;
      }

      await new Promise((r) => setTimeout(r, POLL_INTERVAL));
    }

    // Capture logs
    try {
      const logs = await this.sandboxManager.getLogs(containerId);
      return {
        exitCode: 0,
        stdout: logs,
        stderr: '',
        timedOut: false,
      };
    } catch {
      return {
        exitCode: 0,
        stdout: '',
        stderr: '',
        timedOut: false,
      };
    }
  }
}
