/**
 * CITaskCoordinator — monitors CI status for PRs created by bridge agents
 * and auto-creates fix tasks on failure.
 */

import { v4 as uuidv4 } from 'uuid';
import { pool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';
import { ChannelRouter } from '../channels/ChannelRouter.js';

const logger = new Logger('CITaskCoordinator');

export interface CIEvent {
  repo: string;
  prNumber: number;
  conclusion: 'success' | 'failure';
  logsUrl?: string;
}

interface TaskRecord {
  id: string;
  agentId: string;
  task: string;
}

export class CITaskCoordinator {
  /** Maps prUrl → taskId */
  private readonly prToTask = new Map<string, string>();

  /**
   * Records that a task created a PR. Call this after a bridge agent opens a PR.
   */
  linkPrToTask(taskId: string, prUrl: string): void {
    this.prToTask.set(prUrl, taskId);
    logger.debug(`Linked PR ${prUrl} → task ${taskId}`);
  }

  /**
   * Called when a CI event fires for any PR.
   * Looks up the original task and either creates a fix task (failure) or
   * marks the task context as ci_verified (success).
   */
  async handleCIEvent(event: CIEvent): Promise<void> {
    const prUrl = this.buildPrUrl(event.repo, event.prNumber);
    const taskId = this.prToTask.get(prUrl);

    if (!taskId) {
      logger.debug(`No linked task for PR ${prUrl} — ignoring CI event`);
      return;
    }

    if (event.conclusion === 'failure') {
      await this.handleCIFailure(taskId, event);
    } else {
      await this.handleCISuccess(taskId, event);
    }
  }

  // ── Private ─────────────────────────────────────────────────────────────────

  private buildPrUrl(repo: string, prNumber: number): string {
    return `https://github.com/${repo}/pull/${prNumber}`;
  }

  private async getTaskRecord(taskId: string): Promise<TaskRecord | null> {
    try {
      const { rows } = await pool.query<{
        id: string;
        agent_instance_id: string;
        task: string;
      }>(
        `SELECT id, agent_instance_id, task FROM task_queue WHERE id = $1`,
        [taskId]
      );

      const row = rows[0];
      if (!row) return null;

      return {
        id: row.id,
        agentId: row.agent_instance_id,
        task: row.task,
      };
    } catch (err) {
      logger.warn(`Failed to query task record ${taskId}:`, err);
      return null;
    }
  }

  private async handleCIFailure(taskId: string, event: CIEvent): Promise<void> {
    const original = await this.getTaskRecord(taskId);
    if (!original) {
      logger.warn(`Original task ${taskId} not found — cannot create fix task`);
      return;
    }

    const fixTaskId = uuidv4();
    const fixPrompt =
      `CI failed for PR #${event.prNumber}. ` +
      `Original task: ${original.task}. ` +
      `CI logs: ${event.logsUrl ?? '(no logs URL provided)'}. ` +
      `Fix the failing checks.`;

    const context = JSON.stringify({
      origin: 'ci_fix',
      originalTaskId: taskId,
      prNumber: event.prNumber,
      repo: event.repo,
      ...(event.logsUrl !== undefined ? { logsUrl: event.logsUrl } : {}),
    });

    try {
      await pool.query(
        `INSERT INTO task_queue (id, agent_instance_id, task, priority, context, status, max_retries)
         VALUES ($1, $2, $3, 1, $4, 'queued', 3)`,
        [fixTaskId, original.agentId, fixPrompt, context]
      );

      logger.info(
        `Created CI fix task ${fixTaskId} for agent ${original.agentId} (original task ${taskId})`
      );

      ChannelRouter.getInstance().route({
        id: uuidv4(),
        eventType: 'task.assigned',
        title: `CI fix task assigned`,
        body: fixPrompt.substring(0, 200),
        severity: 'warning',
        metadata: {
          taskId: fixTaskId,
          agentId: original.agentId,
          originalTaskId: taskId,
          prNumber: event.prNumber,
          repo: event.repo,
        },
        timestamp: new Date().toISOString(),
      });
    } catch (err) {
      logger.warn(`Failed to create CI fix task for original task ${taskId}:`, err);
    }
  }

  private async handleCISuccess(taskId: string, event: CIEvent): Promise<void> {
    try {
      await pool.query(
        `UPDATE task_queue
         SET context = context || $2::jsonb
         WHERE id = $1`,
        [
          taskId,
          JSON.stringify({
            ci_verified: true,
            ci_verified_at: new Date().toISOString(),
            prNumber: event.prNumber,
            repo: event.repo,
          }),
        ]
      );

      logger.info(`Marked task ${taskId} as ci_verified (PR #${event.prNumber})`);
    } catch (err) {
      logger.warn(`Failed to update ci_verified on task ${taskId}:`, err);
    }
  }
}
