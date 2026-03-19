/**
 * Metering routes — token usage query API.
 *
 * Endpoints:
 *   GET /api/metering/usage   — aggregated usage with optional filters
 *   GET /api/metering/summary — today's totals across all agents
 *
 * @see docs/epics/04-llm-proxy-and-governance.md § Story 4.4
 */

import { Router } from 'express';
import type { Request, Response } from 'express';
import type { MeteringService } from '../metering/MeteringService.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('MeteringRoute');

export function createMeteringRouter(meteringService: MeteringService): Router {
  const router = Router();

  /**
   * GET /api/metering/usage
   * Query params: agentId, from (ISO8601), to (ISO8601), groupBy (hour|day)
   */
  router.get('/usage', async (req: Request, res: Response) => {
    try {
      const { agentId, from, to, groupBy } = req.query as Record<string, string | undefined>;

      if (groupBy && groupBy !== 'hour' && groupBy !== 'day') {
        res.status(400).json({ error: 'groupBy must be "hour" or "day"' });
        return;
      }

      const rows = await meteringService.getAggregatedUsage({
        ...(agentId ? { agentId } : {}),
        ...(from ? { from } : {}),
        ...(to ? { to } : {}),
        ...(groupBy ? { groupBy: groupBy as 'hour' | 'day' } : {}),
      });

      res.json({ data: rows });
    } catch (err: any) {
      logger.error('Error fetching usage:', err);
      res.status(500).json({ error: 'Internal server error' });
    }
  });

  /**
   * GET /api/metering/summary
   * Returns total usage for the current day across all agents.
   */
  router.get('/summary', async (_req: Request, res: Response) => {
    try {
      const summary = await meteringService.getDailySummary();
      res.json(summary);
    } catch (err: any) {
      logger.error('Error fetching summary:', err);
      res.status(500).json({ error: 'Internal server error' });
    }
  });

  return router;
}
