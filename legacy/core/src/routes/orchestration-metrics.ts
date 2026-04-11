/**
 * Orchestration Metrics Routes
 *
 * GET /api/orchestration/metrics            — all bridge agent metrics
 * GET /api/orchestration/metrics/:agentName — metrics for one bridge agent
 */

import { Router, type Request, type Response } from 'express';
import { OrchestrationMetricsService } from '../services/OrchestrationMetricsService.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('OrchestrationMetricsRouter');

export function createOrchestrationMetricsRouter(): Router {
  const router = Router();
  const service = OrchestrationMetricsService.getInstance();

  // GET /api/orchestration/metrics
  router.get('/metrics', async (_req: Request, res: Response) => {
    const metrics = await service.getAllMetrics();
    return res.json(metrics);
  });

  // GET /api/orchestration/metrics/:agentName
  router.get('/metrics/:agentName', async (req: Request, res: Response) => {
    const agentName = req.params['agentName'] as string;
    const metrics = await service.getMetrics(agentName);
    if (!metrics) {
      logger.info(`No metrics found for agent '${agentName}'`);
      return res.status(404).json({ error: `No metrics found for agent '${agentName}'` });
    }
    return res.json(metrics);
  });

  return router;
}
