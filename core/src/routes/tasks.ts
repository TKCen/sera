/**
 * Task Queue Routes — Stories 5.8 & 5.9
 *
 * Manages the per-agent task queue. Workers poll /tasks/next after completing
 * the current task; sera-core enforces single-task-per-agent concurrency.
 */

import { Router, type Request, type Response } from 'express';
import { v4 as uuidv4 } from 'uuid';
import { pool } from '../lib/database.js';
import { IntercomService } from '../intercom/IntercomService.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('TasksRouter');

// ── Configuration ─────────────────────────────────────────────────────────────

const TASK_RESULT_RETENTION_DAYS = process.env['TASK_RESULT_RETENTION_DAYS']
  ? parseInt(process.env['TASK_RESULT_RETENTION_DAYS'], 10)
  : 30;

/** Max result payload size in KB before truncation. */
const TASK_RESULT_MAX_SIZE_KB = process.env['TASK_RESULT_MAX_SIZE_KB']
  ? parseInt(process.env['TASK_RESULT_MAX_SIZE_KB'], 10)
  : 512;

// ── Types ─────────────────────────────────────────────────────────────────────

interface TaskRow {
  id: string;
  agent_instance_id: string;
  task: string;
  context: unknown;
  status: 'queued' | 'running' | 'completed' | 'failed';
  priority: number;
  retry_count: number;
  max_retries: number;
  created_at: Date;
  started_at: Date | null;
  completed_at: Date | null;
  result: unknown;
  error: string | null;
  usage: unknown;
  thought_stream: unknown;
  result_truncated: boolean;
  exit_reason: string | null;
}

interface AgentRow {
  lifecycle_mode: string;
}

interface RunningRow {
  id: string;
}

// ── Router Factory ────────────────────────────────────────────────────────────

export function createTasksRouter(intercom: IntercomService): Router {
  const router = Router({ mergeParams: true });

  // ── POST /api/agents/:id/tasks — enqueue a new task ───────────────────────
  router.post('/', async (req: Request, res: Response) => {
    const agentId = req.params['id'] as string;
    const { task, context, priority, maxRetries } = req.body as {
      task?: string;
      context?: unknown;
      priority?: number;
      maxRetries?: number;
    };

    if (!task) {
      return res.status(400).json({ error: 'task is required' });
    }

    const agentRow = await getAgentRow(agentId);
    if (!agentRow) return res.status(404).json({ error: 'Agent not found' });
    if (agentRow.lifecycle_mode === 'ephemeral') {
      return res.status(405).json({ error: 'Task queue is not available for ephemeral agents' });
    }

    const taskId = uuidv4();
    const resolvedPriority = priority ?? 100;
    const resolvedMaxRetries = maxRetries ?? 3;

    await pool.query(
      `INSERT INTO task_queue
         (id, agent_instance_id, task, context, priority, max_retries, status)
       VALUES ($1, $2, $3, $4, $5, $6, 'queued')`,
      [taskId, agentId, task, JSON.stringify(context ?? null), resolvedPriority, resolvedMaxRetries]
    );

    const depth = await getQueueDepth(agentId);
    await intercom
      .publish(`agent:${agentId}:status`, {
        event: 'task.queued',
        taskId,
        queueDepth: depth,
      })
      .catch(() => {
        /* best-effort */
      });

    logger.info(`Task ${taskId} enqueued for agent ${agentId}`);
    return res.status(201).json({ taskId, status: 'queued', priority: resolvedPriority });
  });

  // ── GET /api/agents/:id/tasks — list tasks ────────────────────────────────
  router.get('/', async (req: Request, res: Response) => {
    const agentId = req.params['id'] as string;
    const statusFilter = req.query['status'] as string | undefined;

    const agentRow = await getAgentRow(agentId);
    if (!agentRow) return res.status(404).json({ error: 'Agent not found' });
    if (agentRow.lifecycle_mode === 'ephemeral') {
      return res.status(405).json({ error: 'Task queue is not available for ephemeral agents' });
    }

    const conditions = ['agent_instance_id = $1'];
    const params: unknown[] = [agentId];

    if (statusFilter) {
      params.push(statusFilter);
      conditions.push(`status = $${params.length}`);
    }

    const rows = await pool.query<TaskRow>(
      `SELECT * FROM task_queue WHERE ${conditions.join(' AND ')}
       ORDER BY priority ASC, created_at ASC`,
      params as any[]
    );

    return res.json(rows.rows.map(toPublicTask));
  });

  // ── GET /api/agents/:id/tasks/next — worker polls for next task ───────────
  router.get('/next', async (req: Request, res: Response) => {
    const agentId = req.params['id'] as string;

    const agentRow = await getAgentRow(agentId);
    if (!agentRow) return res.status(404).json({ error: 'Agent not found' });
    if (agentRow.lifecycle_mode === 'ephemeral') {
      return res.status(405).json({ error: 'Task queue is not available for ephemeral agents' });
    }

    const client = await pool.connect();
    try {
      await client.query('BEGIN');

      const running = await client.query<RunningRow>(
        `SELECT id FROM task_queue WHERE agent_instance_id = $1 AND status = 'running' LIMIT 1`,
        [agentId]
      );

      if (running.rows.length > 0) {
        await client.query('ROLLBACK');
        return res.status(409).json({
          error: 'Agent already has a running task',
          runningTaskId: running.rows[0]!.id,
        });
      }

      const next = await client.query<TaskRow>(
        `SELECT * FROM task_queue
         WHERE agent_instance_id = $1 AND status = 'queued'
         ORDER BY priority ASC, created_at ASC
         LIMIT 1
         FOR UPDATE SKIP LOCKED`,
        [agentId]
      );

      if (next.rows.length === 0) {
        await client.query('ROLLBACK');
        return res.status(204).send();
      }

      const task = next.rows[0]!;
      await client.query(
        `UPDATE task_queue SET status = 'running', started_at = now() WHERE id = $1`,
        [task.id]
      );
      await client.query('COMMIT');

      logger.info(`Task ${task.id} dispatched to agent ${agentId}`);
      return res.json({
        taskId: task.id,
        task: task.task,
        context: task.context,
        priority: task.priority,
        retryCount: task.retry_count,
        maxRetries: task.max_retries,
      });
    } catch (err) {
      await client.query('ROLLBACK');
      throw err;
    } finally {
      client.release();
    }
  });

  // ── DELETE /api/agents/:id/tasks/:taskId — cancel a queued task ───────────
  router.delete('/:taskId', async (req: Request, res: Response) => {
    const agentId = req.params['id'] as string;
    const taskId = req.params['taskId'] as string;

    const row = await getTaskRow(taskId, agentId);
    if (!row) return res.status(404).json({ error: 'Task not found' });
    if (row.status !== 'queued') {
      return res.status(409).json({ error: `Cannot cancel task in status '${row.status}'` });
    }

    await pool.query(`DELETE FROM task_queue WHERE id = $1`, [taskId]);

    const depth = await getQueueDepth(agentId);
    await intercom
      .publish(`agent:${agentId}:status`, {
        event: 'task.cancelled',
        taskId,
        queueDepth: depth,
      })
      .catch(() => {
        /* best-effort */
      });

    return res.json({ taskId, status: 'cancelled' });
  });

  // ── PATCH /api/agents/:id/tasks/:taskId — update priority ────────────────
  router.patch('/:taskId', async (req: Request, res: Response) => {
    const agentId = req.params['id'] as string;
    const taskId = req.params['taskId'] as string;
    const { priority } = req.body as { priority?: number };

    if (priority === undefined) return res.status(400).json({ error: 'priority is required' });

    const row = await getTaskRow(taskId, agentId);
    if (!row) return res.status(404).json({ error: 'Task not found' });
    if (row.status !== 'queued') {
      return res.status(409).json({ error: `Cannot reprioritize task in status '${row.status}'` });
    }

    await pool.query(`UPDATE task_queue SET priority = $1 WHERE id = $2`, [priority, taskId]);
    return res.json({ taskId, priority });
  });

  // ── POST /api/agents/:id/tasks/:taskId/complete — worker submits result ───
  router.post('/:taskId/complete', async (req: Request, res: Response) => {
    const agentId = req.params['id'] as string;
    const taskId = req.params['taskId'] as string;

    const row = await getTaskRow(taskId, agentId);
    if (!row) return res.status(404).json({ error: 'Task not found' });
    if (row.status !== 'running') {
      return res.status(409).json({ error: `Cannot complete task in status '${row.status}'` });
    }

    const body = req.body as {
      result?: string | null;
      error?: string;
      usage?: { promptTokens: number; completionTokens: number; totalTokens: number };
      thoughtStream?: unknown[];
      exitReason?: string;
    };

    const succeeded = !body.error && body.result !== null && body.result !== undefined;
    const newStatus = succeeded ? 'completed' : 'failed';

    // Check if retry is needed
    if (!succeeded && row.retry_count < row.max_retries) {
      const nextRetry = row.retry_count + 1;
      const backoffMs = Math.pow(2, nextRetry) * 1_000;

      await pool.query(
        `UPDATE task_queue
         SET status = 'queued', retry_count = $1, started_at = NULL, error = $2
         WHERE id = $3`,
        [nextRetry, body.error ?? null, taskId]
      );

      logger.info(
        `Task ${taskId} failed (attempt ${nextRetry}/${row.max_retries}) — retrying in ${backoffMs}ms`
      );

      // DECISION: simple setTimeout for retry scheduling; pg-boss would provide
      // persistence across restarts but requires additional setup.
      setTimeout(() => {
        pool
          .query(`UPDATE task_queue SET status = 'queued' WHERE id = $1 AND status = 'queued'`, [
            taskId,
          ])
          .catch((err: unknown) => {
            logger.error(`Failed to re-queue task ${taskId}:`, err);
          });
      }, backoffMs);

      return res.json({ taskId, status: 'retrying', retryCount: nextRetry });
    }

    // Dead-letter: max retries exhausted on failure
    if (!succeeded && row.retry_count >= row.max_retries) {
      await updateTaskResult(taskId, 'failed', body);
      await intercom
        .publish('system', {
          event: 'system.task-dead-lettered',
          taskId,
          agentId,
          error: body.error,
          retryCount: row.retry_count,
        })
        .catch(() => {
          /* best-effort */
        });
      logger.warn(`Task ${taskId} dead-lettered after ${row.retry_count} retries`);
    } else {
      await updateTaskResult(taskId, newStatus, body);
    }

    const depth = await getQueueDepth(agentId);
    await intercom
      .publish(`agent:${agentId}:status`, {
        event: 'task.completed',
        taskId,
        status: newStatus,
        completedAt: new Date().toISOString(),
        queueDepth: depth,
      })
      .catch(() => {
        /* best-effort */
      });

    return res.json({ taskId, status: newStatus });
  });

  // ── GET /api/agents/:id/tasks/:taskId — full task record (Story 5.9) ──────
  router.get('/:taskId', async (req: Request, res: Response) => {
    const agentId = req.params['id'] as string;
    const taskId = req.params['taskId'] as string;

    const row = await getTaskRow(taskId, agentId);
    if (!row) return res.status(404).json({ error: 'Task not found' });

    return res.json(toPublicTask(row));
  });

  // ── GET /api/agents/:id/tasks/:taskId/result — result payload only ────────
  router.get('/:taskId/result', async (req: Request, res: Response) => {
    const agentId = req.params['id'] as string;
    const taskId = req.params['taskId'] as string;

    const row = await getTaskRow(taskId, agentId);
    if (!row) return res.status(404).json({ error: 'Task not found' });

    if (row.status !== 'completed' && row.status !== 'failed') {
      return res.status(409).json({ error: `Task not yet complete (status: ${row.status})` });
    }

    return res.json({
      taskId: row.id,
      status: row.status,
      result: row.result,
      error: row.error,
      usage: row.usage,
      completedAt: row.completed_at,
      truncated: row.result_truncated,
    });
  });

  return router;
}

// ── Background Retention Job ──────────────────────────────────────────────────

/**
 * Prune task_queue rows older than TASK_RESULT_RETENTION_DAYS in terminal states.
 * Called on startup and periodically (every hour).
 */
export async function pruneOldTaskResults(): Promise<number> {
  const result = await pool.query(
    `DELETE FROM task_queue
     WHERE status IN ('completed', 'failed')
       AND completed_at < now() - INTERVAL '1 day' * $1`,
    [TASK_RESULT_RETENTION_DAYS]
  );
  const pruned = result.rowCount ?? 0;
  if (pruned > 0) {
    logger.info(`Pruned ${pruned} task results older than ${TASK_RESULT_RETENTION_DAYS} days`);
  }
  return pruned;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async function getAgentRow(agentId: string): Promise<AgentRow | null> {
  const r = await pool.query<AgentRow>(`SELECT lifecycle_mode FROM agent_instances WHERE id = $1`, [
    agentId,
  ]);
  return r.rows[0] ?? null;
}

async function getTaskRow(taskId: string, agentId: string): Promise<TaskRow | null> {
  const r = await pool.query<TaskRow>(
    `SELECT * FROM task_queue WHERE id = $1 AND agent_instance_id = $2`,
    [taskId, agentId]
  );
  return r.rows[0] ?? null;
}

async function getQueueDepth(agentId: string): Promise<number> {
  const r = await pool.query<{ count: string }>(
    `SELECT COUNT(*) as count FROM task_queue WHERE agent_instance_id = $1 AND status IN ('queued', 'running')`,
    [agentId]
  );
  return parseInt(r.rows[0]?.count ?? '0', 10);
}

async function updateTaskResult(
  taskId: string,
  status: string,
  body: {
    result?: string | null;
    error?: string;
    usage?: unknown;
    thoughtStream?: unknown[];
    exitReason?: string;
  }
): Promise<void> {
  const maxBytes = TASK_RESULT_MAX_SIZE_KB * 1024;
  let resultValue: unknown = body.result ?? null;
  let truncated = false;

  if (resultValue !== null && typeof resultValue === 'string') {
    const resultStr = JSON.stringify(resultValue);
    if (Buffer.byteLength(resultStr, 'utf-8') > maxBytes) {
      truncated = true;
      resultValue = resultStr.slice(0, maxBytes);
    }
  }

  await pool.query(
    `UPDATE task_queue
     SET status = $1, completed_at = now(), result = $2, error = $3,
         usage = $4, thought_stream = $5, exit_reason = $6, result_truncated = $7
     WHERE id = $8`,
    [
      status,
      JSON.stringify(resultValue),
      body.error ?? null,
      JSON.stringify(body.usage ?? null),
      JSON.stringify(body.thoughtStream ?? null),
      body.exitReason ?? null,
      truncated,
      taskId,
    ]
  );
}

function toPublicTask(row: TaskRow): Record<string, unknown> {
  return {
    id: row.id,
    agentInstanceId: row.agent_instance_id,
    task: row.task,
    context: row.context,
    status: row.status,
    priority: row.priority,
    retryCount: row.retry_count,
    maxRetries: row.max_retries,
    createdAt: row.created_at,
    startedAt: row.started_at,
    completedAt: row.completed_at,
    result: row.result,
    error: row.error,
    usage: row.usage,
    thoughtStream: row.thought_stream,
    exitReason: row.exit_reason,
    resultTruncated: row.result_truncated,
  };
}
