import { Router } from 'express';
import { ScheduleService } from '../services/ScheduleService.js';
import { pool } from '../lib/database.js';

export const createSchedulesRouter = () => {
  const router = Router();
  const scheduleService = ScheduleService.getInstance();

  /**
   * GET /api/schedules - List all schedules
   */
  router.get('/', async (req, res) => {
    try {
      const { agentId } = req.query;
      let query = 'SELECT * FROM schedules';
      const params = [];
      if (agentId) {
        query += ' WHERE agent_instance_id = $1';
        params.push(agentId);
      }
      query += ' ORDER BY created_at DESC';

      const { rows } = await pool.query(query, params);
      // Map snake_case DB columns to camelCase for the frontend
      res.json(rows.map((r: Record<string, unknown>) => ({
        id: r.id,
        agentName: r.agent_name ?? r.agent_instance_id,
        agentInstanceId: r.agent_instance_id,
        name: r.name,
        type: r.type ?? 'cron',
        expression: r.cron_expression ?? r.expression,
        taskPrompt: r.task_prompt,
        status: r.status,
        source: r.source ?? 'api',
        lastRunAt: r.last_run_at,
        lastRunStatus: r.last_run_status,
        lastRunOutput: r.last_run_output,
        nextRunAt: r.next_run_at,
        createdAt: r.created_at,
        updatedAt: r.updated_at,
      })));
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
   * POST /api/schedules/:id/trigger - Manually trigger a schedule
   */
  router.post('/:id/trigger', async (req, res) => {
    try {
      await scheduleService.triggerSchedule(req.params.id);
      res.json({ status: 'triggered' });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  return router;
};
