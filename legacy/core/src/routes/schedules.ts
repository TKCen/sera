import { Router } from 'express';
import { ScheduleService } from '../services/ScheduleService.js';
import { pool } from '../lib/database.js';

/** Sanitize last_run_status to avoid leaking raw SQL errors to the frontend */
function sanitizeRunStatus(value: unknown): string | null {
  if (value == null) return null;
  const str = String(value);
  const sqlKeywords = [
    'column',
    'relation',
    'constraint',
    'violates',
    'syntax error',
    'ERROR:',
    'DETAIL:',
  ];
  if (sqlKeywords.some((kw) => str.toLowerCase().includes(kw.toLowerCase()))) {
    return 'Execution failed';
  }
  return str;
}

export const createSchedulesRouter = (): Router => {
  const router = Router();
  const scheduleService = ScheduleService.getInstance();

  /**
   * GET /api/schedules - List all schedules
   */
  router.get('/', async (req, res) => {
    try {
      const { agentId } = req.query;
      let query = `SELECT s.*, ai.name AS resolved_agent_name
        FROM schedules s
        LEFT JOIN agent_instances ai ON s.agent_instance_id = ai.id`;
      const params = [];
      if (agentId) {
        query += ' WHERE s.agent_instance_id = $1';
        params.push(agentId);
      }
      query += ' ORDER BY s.created_at DESC';

      const { rows } = await pool.query(query, params);
      // Map snake_case DB columns to camelCase for the frontend
      res.json(
        rows.map((r: Record<string, unknown>) => {
          // task is jsonb — may be a string, object, or null
          const rawTask = r.task;
          const taskPrompt =
            typeof rawTask === 'string' ? rawTask : rawTask != null ? JSON.stringify(rawTask) : '';

          return {
            id: r.id,
            agentName: (r.resolved_agent_name as string) ?? r.agent_name ?? r.agent_instance_id,
            agentInstanceId: r.agent_instance_id,
            name: r.name,
            type: r.type ?? 'cron',
            expression: r.cron ?? r.expression,
            taskPrompt,
            status: r.status,
            source: r.source ?? 'api',
            category: r.category ?? null,
            lastRunAt: r.last_run_at ?? r.last_run,
            lastRunStatus: sanitizeRunStatus(r.last_run_status),
            nextRunAt: r.next_run_at,
            createdAt: r.created_at,
            updatedAt: r.updated_at,
          };
        })
      );
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * POST /api/schedules - Create a new schedule
   */
  router.post('/', async (req, res) => {
    try {
      const schedule = await scheduleService.createSchedule(req.body);
      res.status(201).json(schedule);
    } catch (err: unknown) {
      res.status(400).json({ error: (err as Error).message });
    }
  });

  /**
   * GET /api/schedules/runs - List schedule-triggered task runs
   * Query params: category, scheduleId, agentId, limit (default 50)
   * NOTE: Must be registered before /:id to avoid Express matching "runs" as an id.
   */
  router.get('/runs', async (req, res) => {
    try {
      const { category, scheduleId, agentId, limit: limitStr } = req.query;
      const limit = Math.min(Math.max(parseInt(String(limitStr || '50'), 10) || 50, 1), 200);

      const conditions: string[] = ["tq.context->'schedule' IS NOT NULL"];
      const params: unknown[] = [];
      let paramIdx = 1;

      if (category) {
        conditions.push(`s.category = $${paramIdx++}`);
        params.push(category);
      }
      if (scheduleId) {
        conditions.push(`(tq.context->'schedule'->>'scheduleId') = $${paramIdx++}`);
        params.push(scheduleId);
      }
      if (agentId) {
        conditions.push(`tq.agent_instance_id = $${paramIdx++}`);
        params.push(agentId);
      }

      params.push(limit);

      const { rows } = await pool.query(
        `SELECT tq.id AS task_id, tq.status, tq.result, tq.error, tq.usage,
                tq.exit_reason, tq.created_at, tq.started_at, tq.completed_at,
                tq.context->'schedule'->>'scheduleId' AS schedule_id,
                tq.context->'schedule'->>'scheduleName' AS schedule_name,
                tq.context->'schedule'->>'category' AS schedule_category,
                tq.context->'schedule'->>'firedAt' AS fired_at,
                s.name AS current_schedule_name, s.category AS current_category
         FROM task_queue tq
         LEFT JOIN schedules s ON (tq.context->'schedule'->>'scheduleId')::uuid = s.id
         WHERE ${conditions.join(' AND ')}
         ORDER BY tq.created_at DESC
         LIMIT $${paramIdx}`,
        params
      );

      res.json(
        rows.map((r: Record<string, unknown>) => ({
          taskId: r.task_id,
          scheduleId: r.schedule_id,
          scheduleName: r.current_schedule_name ?? r.schedule_name,
          scheduleCategory: r.current_category ?? r.schedule_category,
          status: r.status,
          result: r.result,
          error: r.error,
          usage: r.usage,
          exitReason: r.exit_reason,
          firedAt: r.fired_at,
          startedAt: r.started_at,
          completedAt: r.completed_at,
          createdAt: r.created_at,
        }))
      );
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * GET /api/schedules/:id - Get a schedule by ID
   */
  router.get('/:id', async (req, res) => {
    try {
      const { rows } = await pool.query('SELECT * FROM schedules WHERE id = $1', [req.params.id]);
      if (rows.length === 0) return res.status(404).json({ error: 'Schedule not found' });
      res.json(rows[0]);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * PATCH /api/schedules/:id - Update a schedule
   */
  router.patch('/:id', async (req, res) => {
    try {
      // Check if manifest schedule
      const { rows } = await pool.query('SELECT source FROM schedules WHERE id = $1', [
        req.params.id,
      ]);
      if (rows.length === 0) return res.status(404).json({ error: 'Schedule not found' });

      const schedule = await scheduleService.updateSchedule(req.params.id, req.body);
      res.json(schedule);
    } catch (err: unknown) {
      res.status(400).json({ error: (err as Error).message });
    }
  });

  /**
   * DELETE /api/schedules/:id - Delete a schedule
   */
  router.delete('/:id', async (req, res) => {
    try {
      // Check if manifest schedule
      const { rows } = await pool.query('SELECT source FROM schedules WHERE id = $1', [
        req.params.id,
      ]);
      if (rows.length === 0) return res.status(404).json({ error: 'Schedule not found' });

      if (rows[0].source === 'manifest') {
        return res.status(403).json({
          error:
            'Manifest-declared schedules cannot be deleted via API. Remove it from the agent manifest instead.',
        });
      }

      await scheduleService.deleteSchedule(req.params.id);
      res.status(204).send();
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * GET /api/schedules/:id/runs - List runs for a specific schedule
   * Query params: limit (default 20)
   */
  router.get('/:id/runs', async (req, res) => {
    try {
      const limit = Math.min(Math.max(parseInt(String(req.query.limit || '20'), 10) || 20, 1), 200);

      const { rows } = await pool.query(
        `SELECT tq.id AS task_id, tq.status, tq.result, tq.error, tq.usage,
                tq.exit_reason, tq.created_at, tq.started_at, tq.completed_at,
                tq.context->'schedule'->>'firedAt' AS fired_at
         FROM task_queue tq
         WHERE (tq.context->'schedule'->>'scheduleId') = $1
         ORDER BY tq.created_at DESC
         LIMIT $2`,
        [req.params.id, limit]
      );

      res.json(
        rows.map((r: Record<string, unknown>) => ({
          taskId: r.task_id,
          status: r.status,
          result: r.result,
          error: r.error,
          usage: r.usage,
          exitReason: r.exit_reason,
          firedAt: r.fired_at,
          startedAt: r.started_at,
          completedAt: r.completed_at,
          createdAt: r.created_at,
        }))
      );
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * POST /api/schedules/:id/trigger - Manually trigger a schedule
   */
  router.post('/:id/trigger', async (req, res) => {
    try {
      const force = req.query.force === 'true';
      const result = await scheduleService.triggerSchedule(req.params.id, force);
      if (result.status === 'skipped') {
        return res.status(409).json(result);
      }
      res.json(result);
    } catch (err: unknown) {
      const status = (err as { status?: number }).status || 500;
      res.status(status).json({ error: (err as Error).message });
    }
  });

  return router;
};
