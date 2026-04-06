/**
 * Traces Routes — Interaction trace persistence API (Epic 30, Story 30.1)
 *
 * Endpoints:
 *   GET  /api/traces              — list traces (optional ?agentId=, ?limit=, ?offset=)
 *   GET  /api/traces/:id          — get trace by ID
 *   GET  /api/traces/session/:agentId/:sessionId — get traces by agent+session
 *   DELETE /api/traces/:id        — delete a trace
 */

import { Router } from 'express';
import type { Request, Response } from 'express';
import { TraceService } from '../services/TraceService.js';

export function createTracesRouter(): Router {
  const router = Router();
  const traceService = TraceService.getInstance();

  /**
   * GET /api/traces
   * Query params: agentId?, limit? (default 50), offset? (default 0)
   */
  router.get('/list', async (req: Request, res: Response) => {
    try {
      const agentId = typeof req.query['agentId'] === 'string' ? req.query['agentId'] : undefined;
      const limit = Math.min(parseInt(String(req.query['limit'] ?? '50'), 10) || 50, 200);
      const offset = parseInt(String(req.query['offset'] ?? '0'), 10) || 0;

      const traces = await traceService.listTraces(agentId, limit, offset);
      res.json(traces);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * GET /api/traces/session/:agentId/:sessionId
   */
  router.get('/session/:agentId/:sessionId', async (req: Request, res: Response) => {
    try {
      const { agentId, sessionId } = req.params as { agentId: string; sessionId: string };
      const traces = await traceService.getTracesBySession(agentId, sessionId);
      res.json(traces);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * GET /api/traces/:id
   */
  router.get('/:id', async (req: Request, res: Response) => {
    try {
      const { id } = req.params as { id: string };
      const trace = await traceService.getTrace(id);
      if (!trace) {
        res.status(404).json({ error: 'Trace not found' });
        return;
      }
      res.json(trace);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * DELETE /api/traces/:id
   */
  router.delete('/:id', async (req: Request, res: Response) => {
    try {
      const { id } = req.params as { id: string };
      const deleted = await traceService.deleteTrace(id);
      if (!deleted) {
        res.status(404).json({ error: 'Trace not found' });
        return;
      }
      res.status(204).end();
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  return router;
}
