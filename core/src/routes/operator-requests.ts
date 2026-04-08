/**
 * Operator Requests Routes
 *
 * Bidirectional communication channel between SERA agents and operators (human or Claude Code).
 * Agents create requests when they need something they cannot do themselves.
 */

import { Router } from 'express';
import { pool } from '../lib/database.js';
import type { IntercomService } from '../intercom/IntercomService.js';
import { requireRole } from '../auth/authMiddleware.js';
import { z } from 'zod';
import { QueryBuilder } from '../lib/query-builder.js';

const OperatorRequestQuerySchema = z.object({
  status: z.string().optional(),
  agentId: z.string().optional(),
  limit: z.coerce.number().int().min(1).max(200).default(50),
});

export function createOperatorRequestsRouter(intercom?: IntercomService): Router {
  const router = Router();

  /**
   * GET /api/operator-requests/pending/count — Count pending requests (for badges)
   * NOTE: Registered before parameterised routes to avoid Express 5 shadowing.
   */
  router.get('/pending/count', requireRole(['admin', 'operator']), async (_req, res) => {
    try {
      const { rows } = await pool.query(
        "SELECT COUNT(*)::int AS count FROM operator_requests WHERE status = 'pending'"
      );
      res.json({ count: rows[0]!.count });
    } catch (err) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * GET /api/operator-requests — List operator requests
   * Query params: status, agentId, limit
   */
  router.get('/', requireRole(['admin', 'operator']), async (req, res) => {
    try {
      const { status, agentId, limit } = OperatorRequestQuerySchema.parse(req.query);

      const qb = new QueryBuilder();

      if (status) {
        qb.addCondition('status = ?', status);
      }
      if (agentId) {
        qb.addCondition('agent_id = ?', agentId);
      }

      const whereClause = qb.buildWhere();
      const limitPlaceholder = qb.addParam(limit);

      const query = `SELECT * FROM operator_requests${whereClause} ORDER BY created_at DESC LIMIT ${limitPlaceholder}`;

      const { rows } = await pool.query(query, qb.getParams());

      // Map snake_case to camelCase for frontend
      res.json(
        rows.map((r: Record<string, unknown>) => ({
          id: r.id,
          agentId: r.agent_id,
          agentName: r.agent_name,
          type: r.type,
          title: r.title,
          payload: r.payload,
          status: r.status,
          response: r.response,
          createdAt: r.created_at,
          resolvedAt: r.resolved_at,
        }))
      );
    } catch (err) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * POST /api/operator-requests/:id/respond — Respond to a request
   * Body: { action: 'approved' | 'rejected' | 'resolved', response?: string | object }
   */
  router.post('/:id/respond', async (req, res) => {
    try {
      const { id } = req.params;
      const { action, response } = req.body as {
        action?: string;
        response?: unknown;
      };

      if (!action || !['approved', 'rejected', 'resolved'].includes(action)) {
        return res
          .status(400)
          .json({ error: 'action must be one of: approved, rejected, resolved' });
      }

      const responseJson =
        response != null
          ? typeof response === 'string'
            ? JSON.stringify({ message: response })
            : JSON.stringify(response)
          : null;

      const { rows, rowCount } = await pool.query(
        `UPDATE operator_requests
         SET status = $1, response = $2, resolved_at = NOW()
         WHERE id = $3 AND status = 'pending'
         RETURNING *`,
        [action, responseJson, id]
      );

      if (rowCount === 0) {
        return res.status(404).json({ error: 'Request not found or already resolved' });
      }

      const row = rows[0] as Record<string, unknown>;

      // Notify via Centrifugo so agents see the response in real-time
      if (intercom) {
        intercom
          .publishSystem('operator_request_response', {
            requestId: id,
            agentId: row.agent_id,
            action,
            response: responseJson ? JSON.parse(responseJson) : null,
            timestamp: new Date().toISOString(),
          })
          .catch(() => {});
      }

      res.json({
        id: row.id,
        agentId: row.agent_id,
        status: action,
        response: row.response,
        resolvedAt: row.resolved_at,
      });
    } catch (err) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  return router;
}
