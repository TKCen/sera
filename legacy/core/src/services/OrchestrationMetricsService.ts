/**
 * OrchestrationMetricsService — per-bridge-agent task queue metrics.
 *
 * Computes aggregated metrics from the task_queue table joined with
 * agent_instances. Intended for the /api/orchestration/metrics endpoints.
 */

import { pool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('OrchestrationMetricsService');

// ── Types ─────────────────────────────────────────────────────────────────────

export interface ToolMetrics {
  agentName: string;
  totalTasks: number;
  completedTasks: number;
  failedTasks: number;
  successRate: number;
  avgDurationMs: number;
  queuedTasks: number;
  lastTaskAt: string | null;
}

interface MetricsRow {
  agent_name: string;
  total_tasks: string;
  completed_tasks: string;
  failed_tasks: string;
  avg_duration_ms: string | null;
  queued_tasks: string;
  last_task_at: Date | null;
}

// ── Service ───────────────────────────────────────────────────────────────────

export class OrchestrationMetricsService {
  private static instance: OrchestrationMetricsService;

  private constructor() {}

  static getInstance(): OrchestrationMetricsService {
    if (!OrchestrationMetricsService.instance) {
      OrchestrationMetricsService.instance = new OrchestrationMetricsService();
    }
    return OrchestrationMetricsService.instance;
  }

  /**
   * Get metrics for all bridge agents (agents present in task_queue).
   */
  async getAllMetrics(): Promise<ToolMetrics[]> {
    const result = await pool.query<MetricsRow>(`
      SELECT
        ai.name                                                          AS agent_name,
        COUNT(tq.id)                                                     AS total_tasks,
        COUNT(tq.id) FILTER (WHERE tq.status = 'completed')             AS completed_tasks,
        COUNT(tq.id) FILTER (WHERE tq.status = 'failed')                AS failed_tasks,
        AVG(
          EXTRACT(EPOCH FROM (tq.completed_at - tq.created_at)) * 1000
        ) FILTER (WHERE tq.status IN ('completed', 'failed')
                    AND tq.completed_at IS NOT NULL)                     AS avg_duration_ms,
        COUNT(tq.id) FILTER (WHERE tq.status = 'queued')                AS queued_tasks,
        MAX(tq.created_at)                                               AS last_task_at
      FROM agent_instances ai
      INNER JOIN task_queue tq ON tq.agent_instance_id = ai.id
      GROUP BY ai.id, ai.name
      ORDER BY ai.name
    `);

    logger.info(`Fetched metrics for ${result.rows.length} agents`);
    return result.rows.map(toToolMetrics);
  }

  /**
   * Get metrics for a single bridge agent by name.
   * Returns null if the agent has no tasks or does not exist.
   */
  async getMetrics(agentName: string): Promise<ToolMetrics | null> {
    const result = await pool.query<MetricsRow>(
      `
      SELECT
        ai.name                                                          AS agent_name,
        COUNT(tq.id)                                                     AS total_tasks,
        COUNT(tq.id) FILTER (WHERE tq.status = 'completed')             AS completed_tasks,
        COUNT(tq.id) FILTER (WHERE tq.status = 'failed')                AS failed_tasks,
        AVG(
          EXTRACT(EPOCH FROM (tq.completed_at - tq.created_at)) * 1000
        ) FILTER (WHERE tq.status IN ('completed', 'failed')
                    AND tq.completed_at IS NOT NULL)                     AS avg_duration_ms,
        COUNT(tq.id) FILTER (WHERE tq.status = 'queued')                AS queued_tasks,
        MAX(tq.created_at)                                               AS last_task_at
      FROM agent_instances ai
      INNER JOIN task_queue tq ON tq.agent_instance_id = ai.id
      WHERE ai.name = $1
      GROUP BY ai.id, ai.name
      `,
      [agentName]
    );

    if (result.rows.length === 0) return null;
    return toToolMetrics(result.rows[0]!);
  }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function toToolMetrics(row: MetricsRow): ToolMetrics {
  const completed = parseInt(row.completed_tasks, 10);
  const failed = parseInt(row.failed_tasks, 10);
  const terminal = completed + failed;
  const successRate = terminal > 0 ? completed / terminal : 0;

  return {
    agentName: row.agent_name,
    totalTasks: parseInt(row.total_tasks, 10),
    completedTasks: completed,
    failedTasks: failed,
    successRate,
    avgDurationMs: row.avg_duration_ms !== null ? parseFloat(row.avg_duration_ms) : 0,
    queuedTasks: parseInt(row.queued_tasks, 10),
    lastTaskAt: row.last_task_at ? row.last_task_at.toISOString() : null,
  };
}
