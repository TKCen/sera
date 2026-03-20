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
      res.json(rows);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /**
   * POST /api/schedules - Create a new schedule
   */
  router.post('/', async (req, res) => {
    try {
      const schedule = await scheduleService.createSchedule(req.body);
      res.status(201).json(schedule);
    } catch (err: any) {
      res.status(400).json({ error: err.message });
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
    } catch (err: any) {
      res.status(500).json({ error: err.message });
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
    } catch (err: any) {
      res.status(400).json({ error: err.message });
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
        return res
          .status(403)
          .json({
            error:
              'Manifest-declared schedules cannot be deleted via API. Remove it from the agent manifest instead.',
          });
      }

      await scheduleService.deleteSchedule(req.params.id);
      res.status(204).send();
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /**
   * POST /api/schedules/:id/trigger - Manually trigger a schedule
   */
  router.post('/:id/trigger', async (req, res) => {
    try {
      await scheduleService.triggerSchedule(req.params.id);
      res.json({ status: 'triggered' });
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  return router;
};
