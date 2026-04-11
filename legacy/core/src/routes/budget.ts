import { Router } from 'express';
import type { Request, Response } from 'express';
import { query } from '../lib/database.js';
import type { MeteringService } from '../metering/MeteringService.js';
import { DEFAULT_HOURLY_QUOTA, DEFAULT_DAILY_QUOTA } from '../metering/MeteringService.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('BudgetRoute');

export function createBudgetRouter(meteringService?: MeteringService): Router {
  const router = Router();

  /**
   * GET /api/budget
   * Global totals (sum of all tokens across all agents, grouped by day for the last 7 days)
   */
  router.get('/', async (_req: Request, res: Response) => {
    try {
      // Use token_usage only — usage_events also receives every insert, so
      // UNION ALL would double-count every request (#751).
      const result = await query(
        `SELECT
           DATE_TRUNC('day', created_at) AS date,
           SUM(total_tokens) AS total_tokens
         FROM token_usage
         WHERE created_at >= NOW() - INTERVAL '7 days'
         GROUP BY DATE_TRUNC('day', created_at)
         ORDER BY date ASC`
      );

      const usage = result.rows.map((row) => ({
        date: (row.date as Date).toISOString().split('T')[0],
        totalTokens: parseInt(row.total_tokens as string, 10),
      }));

      res.json({ usage });
    } catch (err: unknown) {
      logger.error('Error fetching global budget:', err);
      res.status(500).json({ error: 'Internal server error' });
    }
  });

  /**
   * GET /api/budget/agents
   * Per-agent rankings (sum tokens per agent, ordered by total descending)
   */
  router.get('/agents', async (_req: Request, res: Response) => {
    try {
      // Use token_usage only — see #751.
      const result = await query(
        `SELECT
           agent_id,
           SUM(total_tokens) AS total_tokens
         FROM token_usage
         GROUP BY agent_id
         ORDER BY total_tokens DESC`
      );

      const rankings = result.rows.map((row) => ({
        agentId: row.agent_id as string,
        totalTokens: parseInt(row.total_tokens as string, 10),
      }));

      res.json({ rankings });
    } catch (err: unknown) {
      logger.error('Error fetching agent rankings:', err);
      res.status(500).json({ error: 'Internal server error' });
    }
  });

  /**
   * GET /api/budget/agents/:id
   * Single agent's usage history (last 7 days, grouped by day)
   */
  router.get('/agents/:id', async (req: Request, res: Response) => {
    try {
      const agentId = req.params.id;
      // Use token_usage only — see #751.
      const result = await query(
        `SELECT
           DATE_TRUNC('day', created_at) AS date,
           SUM(total_tokens) AS total_tokens
         FROM token_usage
         WHERE agent_id = $1 AND created_at >= NOW() - INTERVAL '7 days'
         GROUP BY DATE_TRUNC('day', created_at)
         ORDER BY date ASC`,
        [agentId]
      );

      const usage = result.rows.map((row) => ({
        date: (row.date as Date).toISOString().split('T')[0],
        totalTokens: parseInt(row.total_tokens as string, 10),
      }));

      res.json({ agentId, usage });
    } catch (err: unknown) {
      logger.error(`Error fetching budget for agent ${req.params.id}:`, err);
      res.status(500).json({ error: 'Internal server error' });
    }
  });

  /**
   * GET /api/budget/agents/:id/budget
   * Returns real-time hourly and daily budget status for a specific agent.
   * Story 4.3: current usage vs limits for both hourly and daily windows.
   */
  router.get('/agents/:id/budget', async (req: Request, res: Response) => {
    if (!meteringService) {
      res.status(503).json({ error: 'MeteringService not available' });
      return;
    }
    try {
      const agentId = String(req.params['id']);
      const status = await meteringService.checkBudget(agentId);
      res.json({
        agentId,
        ...status,
        hourlyUnlimited: status.hourlyQuota === 0,
        dailyUnlimited: status.dailyQuota === 0,
      });
    } catch (err: unknown) {
      logger.error(`Error fetching budget for agent ${String(req.params['id'])}:`, err);
      res.status(500).json({ error: 'Internal server error' });
    }
  });

  /**
   * PATCH /api/budget/agents/:id/budget
   * Update an agent's token quotas (hourly and/or daily).
   * Upserts into token_quotas so it works even if no row exists yet.
   */
  router.patch('/agents/:id/budget', async (req: Request, res: Response) => {
    try {
      const agentId = String(req.params['id']);
      const { maxLlmTokensPerHour, maxLlmTokensPerDay } = req.body as {
        maxLlmTokensPerHour?: number | null;
        maxLlmTokensPerDay?: number | null;
      };

      // Resolve values: null or 0 → 0 (unlimited), undefined → keep existing (represented as null here)
      const resolveQuota = (val: number | null | undefined): number | null => {
        if (val === undefined) return null; // not in request — keep existing
        if (val === null || val === 0) return 0; // explicit unlimited
        return val;
      };

      const hourly = resolveQuota(maxLlmTokensPerHour);
      const daily = resolveQuota(maxLlmTokensPerDay);

      // Nothing to update if neither field was provided
      if (hourly === null && daily === null) {
        res.json({ success: true });
        return;
      }

      // Read current values so the INSERT path (new row) preserves unspecified fields
      // rather than falling back to process-level defaults (#750).
      const existing = await query(
        `SELECT max_tokens_per_hour, max_tokens_per_day FROM token_quotas WHERE agent_id = $1`,
        [agentId]
      );
      const currentHourly: number =
        existing.rows[0]?.max_tokens_per_hour != null
          ? parseInt(String(existing.rows[0].max_tokens_per_hour), 10)
          : DEFAULT_HOURLY_QUOTA;
      const currentDaily: number =
        existing.rows[0]?.max_tokens_per_day != null
          ? parseInt(String(existing.rows[0].max_tokens_per_day), 10)
          : DEFAULT_DAILY_QUOTA;

      // Upsert with explicit casts — PostgreSQL needs type hints when params may be null.
      await query(
        `INSERT INTO token_quotas (agent_id, max_tokens_per_hour, max_tokens_per_day, source, updated_at)
         VALUES ($1, COALESCE($2::int, $3::int), COALESCE($4::int, $5::int), 'operator', NOW())
         ON CONFLICT (agent_id)
         DO UPDATE SET
           max_tokens_per_hour = COALESCE($2::int, token_quotas.max_tokens_per_hour),
           max_tokens_per_day  = COALESCE($4::int, token_quotas.max_tokens_per_day),
           source = 'operator',
           updated_at = NOW()`,
        [agentId, hourly, currentHourly, daily, currentDaily]
      );

      logger.info(
        `Budget updated for agent=${agentId} hourly=${hourly ?? 'unchanged'} daily=${daily ?? 'unchanged'}`
      );
      res.json({ success: true });
    } catch (err: unknown) {
      logger.error(`Error updating budget for agent ${String(req.params['id'])}:`, err);
      res.status(500).json({ error: 'Internal server error' });
    }
  });

  /**
   * POST /api/budget/agents/:id/budget/reset
   * Reset an agent's usage counters by deleting their token_usage rows.
   */
  router.post('/agents/:id/budget/reset', async (req: Request, res: Response) => {
    try {
      const agentId = String(req.params['id']);

      await query(`DELETE FROM token_usage WHERE agent_id = $1`, [agentId]);

      logger.info(`Budget counters reset for agent=${agentId}`);
      res.json({ success: true });
    } catch (err: unknown) {
      logger.error(`Error resetting budget for agent ${String(req.params['id'])}:`, err);
      res.status(500).json({ error: 'Internal server error' });
    }
  });

  return router;
}
