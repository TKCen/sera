/**
 * SubagentRunner — manages subagent container lifecycle.
 *
 * Subagents are short-lived agents spawned by a parent agent to handle
 * delegated tasks. They share the parent's workspace volume and report
 * results back when complete.
 *
 * @see sera/docs/reimplementation/agent-workspace-architecture.md § The Subagent
 */

import type { AgentManifest } from './manifest/types.js';
import type { SandboxInfo } from '../sandbox/types.js';
import { SandboxManager } from '../sandbox/SandboxManager.js';
import { TierPolicy, PolicyViolationError } from '../sandbox/TierPolicy.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('SubagentRunner');

// ── SubagentResult ──────────────────────────────────────────────────────────────

export interface SubagentResult {
  subagentRole: string;
  containerId: string;
  status: 'completed' | 'failed' | 'timeout';
  output?: string;
  error?: string;
  durationMs: number;
}

// ── SubagentRunner ──────────────────────────────────────────────────────────────

export class SubagentRunner {
  private sandboxManager: SandboxManager;

  constructor(sandboxManager: SandboxManager) {
    this.sandboxManager = sandboxManager;
  }

  /**
   * Spawn a subagent container for a parent agent.
   *
   * Validates:
   * - The subagent role is in the parent's `subagents.allowed` list
   * - The `maxInstances` limit has not been reached
   *
   * The subagent container:
   * - Mounts the parent's workspace volume (read-write)
   * - Receives the task description via environment variable
   * - Gets its own AGENT.yaml-derived configuration
   */
  async spawnSubagent(
    parentManifest: AgentManifest,
    childRole: string,
    task: string,
    options?: { image?: string }
  ): Promise<SandboxInfo> {
    const parentName = parentManifest.metadata.name;

    // ── Validate role is allowed ──────────────────────────────────────────
    const allowedEntry = parentManifest.subagents?.allowed?.find((s) => s.role === childRole);

    if (!allowedEntry) {
      throw new PolicyViolationError(
        `Agent "${parentName}" is not allowed to spawn subagent role "${childRole}"`,
        parentName,
        'subagent_role_not_allowed'
      );
    }

    // ── Check instance limit ──────────────────────────────────────────────
    const currentCount = this.sandboxManager.countSubagents(parentName, childRole);
    TierPolicy.checkInstanceLimit(parentManifest, childRole, currentCount);

    // ── Check approval requirement ────────────────────────────────────────
    if (allowedEntry.requiresApproval) {
      logger.info(
        `Subagent "${childRole}" for "${parentName}" requires human approval — auto-approved for now`
      );
      // Future: integrate with approval workflow
    }

    // ── Spawn via SandboxManager ──────────────────────────────────────────
    const image = options?.image ?? 'node:20-slim';

    const sandbox = await this.sandboxManager.spawn(parentManifest, {
      agentName: parentName,
      type: 'subagent',
      image,
      subagentRole: childRole,
      task,
      env: {
        SERA_PARENT_AGENT: parentName,
        SERA_SUBAGENT_ROLE: childRole,
        SERA_TASK: task,
        SERA_CIRCLE: parentManifest.metadata.circle ?? '',
      },
    });

    logger.info(
      `Spawned subagent "${childRole}" for "${parentName}" → ${sandbox.containerId.substring(0, 12)}`
    );

    return sandbox;
  }

  /**
   * Get all active subagents for a parent agent.
   */
  getActiveSubagents(parentName: string): SandboxInfo[] {
    return this.sandboxManager
      .listContainers(parentName)
      .filter((c) => c.type === 'subagent' && c.parentAgent === parentName);
  }

  /**
   * Wait for a subagent to complete and return its result.
   */
  async waitForSubagent(
    parentManifest: AgentManifest,
    containerId: string,
    timeoutMs: number = 120_000
  ): Promise<SubagentResult> {
    const startTime = Date.now();
    const sandbox = this.sandboxManager.listContainers().find((c) => c.containerId === containerId);

    if (!sandbox) {
      return {
        subagentRole: 'unknown',
        containerId,
        status: 'failed',
        error: 'Container not found',
        durationMs: Date.now() - startTime,
      };
    }

    // Poll until complete or timeout
    const POLL_INTERVAL = 1000;
    while (Date.now() - startTime < timeoutMs) {
      const current = this.sandboxManager
        .listContainers()
        .find((c) => c.containerId === containerId);

      if (!current || current.status === 'stopped') {
        // Get output
        let output = '';
        try {
          output = await this.sandboxManager.getLogs(containerId);
        } catch {
          // Container may have been auto-removed
        }

        // Clean up
        try {
          await this.sandboxManager.remove(parentManifest, containerId);
        } catch {
          // Already removed
        }

        return {
          subagentRole: sandbox.subagentRole ?? 'unknown',
          containerId,
          status: 'completed',
          output,
          durationMs: Date.now() - startTime,
        };
      }

      await new Promise((r) => setTimeout(r, POLL_INTERVAL));
    }

    // Timeout — force remove
    try {
      await this.sandboxManager.remove(parentManifest, containerId);
    } catch {
      // Best-effort cleanup
    }

    return {
      subagentRole: sandbox.subagentRole ?? 'unknown',
      containerId,
      status: 'timeout',
      error: `Subagent timed out after ${timeoutMs}ms`,
      durationMs: Date.now() - startTime,
    };
  }
}
