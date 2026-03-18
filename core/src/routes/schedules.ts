import { Router } from 'express';
import { v4 as uuidv4 } from 'uuid';
import { query } from '../lib/database.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('SchedulesRouter');

export function createSchedulesRouter() {
  const router = Router();

  // ── List all schedules ──────────────────────────────────────────────────
  router.get('/', async (req, res) => {
    try {
      const agentId = req.query.agentId as string | undefined;
      let sql = `
        SELECT s.*, ai.name as agent_name, ai.template_name
        FROM schedules s
        JOIN agent_instances ai ON s.agent_id = ai.id
      `;
      const params: any[] = [];

      if (agentId) {
        sql += ` WHERE s.agent_id = $1`;
        params.push(agentId);
      }

      sql += ` ORDER BY s.created_at DESC`;

      const result = await query(sql, params);
      res.json(result.rows);
    } catch (err: any) {
      logger.error('Error listing schedules:', err);
      res.status(500).json({ error: err.message });
    }
  });

  // ── Create a new schedule ───────────────────────────────────────────────
  router.post('/', async (req, res) => {
    try {
      const { agentId, name, cron, task } = req.body;
      if (!agentId || !name || !cron || !task) {
        return res.status(400).json({ error: 'agentId, name, cron, and task are required' });
      }

      const id = uuidv4();
      const now = new Date().toISOString();

      await query(
        `INSERT INTO schedules (id, agent_id, name, cron, task, status, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, 'active', $6, $6)`,
        [id, agentId, name, cron, JSON.stringify(task), now],
      );

      const result = await query(
        `SELECT s.*, ai.name as agent_name, ai.template_name FROM schedules s
         JOIN agent_instances ai ON s.agent_id = ai.id
         WHERE s.id = $1`,
        [id]
      );

      res.status(201).json(result.rows[0]);
    } catch (err: any) {
      logger.error('Error creating schedule:', err);
      res.status(500).json({ error: err.message });
    }
  });

  // ── Get schedule detail ─────────────────────────────────────────────────
  router.get('/:id', async (req, res) => {
    try {
      const result = await query(
        `SELECT s.*, ai.name as agent_name, ai.template_name
         FROM schedules s
         JOIN agent_instances ai ON s.agent_id = ai.id
         WHERE s.id = $1`,
        [req.params.id],
      );

      if (result.rows.length === 0) {
        return res.status(404).json({ error: 'Schedule not found' });
      }

      res.json(result.rows[0]);
    } catch (err: any) {
      logger.error('Error getting schedule:', err);
      res.status(500).json({ error: err.message });
    }
  });

  // ── Update a schedule ───────────────────────────────────────────────────
  router.put('/:id', async (req, res) => {
    try {
      const { name, cron, task, status } = req.body;
      const { id } = req.params;

      const currentResult = await query('SELECT * FROM schedules WHERE id = $1', [id]);
      if (currentResult.rows.length === 0) {
        return res.status(404).json({ error: 'Schedule not found' });
      }

      const current = currentResult.rows[0];
      const newName = name ?? current.name;
      const newCron = cron ?? current.cron;
      const newTask = task ? JSON.stringify(task) : current.task;
      const newStatus = status ?? current.status;

      await query(
        `UPDATE schedules
         SET name = $1, cron = $2, task = $3, status = $4, updated_at = NOW()
         WHERE id = $5`,
        [newName, newCron, newTask, newStatus, id],
      );

      const updatedResult = await query(
        `SELECT s.*, ai.name as agent_name, ai.template_name FROM schedules s
         JOIN agent_instances ai ON s.agent_id = ai.id
         WHERE s.id = $1`,
        [id]
      );

      res.json(updatedResult.rows[0]);
    } catch (err: any) {
      logger.error('Error updating schedule:', err);
      res.status(500).json({ error: err.message });
    }
  });

  // ── Delete a schedule ───────────────────────────────────────────────────
  router.delete('/:id', async (req, res) => {
    try {
      const { id } = req.params;
      const result = await query('DELETE FROM schedules WHERE id = $1', [id]);

      if (result.rowCount === 0) {
        return res.status(404).json({ error: 'Schedule not found' });
      }

      res.status(204).send();
    } catch (err: any) {
      logger.error('Error deleting schedule:', err);
      res.status(500).json({ error: err.message });
    }
  });

  return router;
}
