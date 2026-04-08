import { Router } from 'express';
import { AuditService } from '../audit/AuditService.js';
import { requireRole } from '../auth/authMiddleware.js';
import { z } from 'zod';
import { QueryBuilder } from '../lib/query-builder.js';

const AuditQuerySchema = z.object({
  actorId: z.string().optional(),
  eventType: z.string().optional(),
  from: z.string().optional(),
  to: z.string().optional(),
  limit: z.coerce.number().int().min(1).max(200).default(50),
  offset: z.coerce.number().int().min(0).default(0),
  principalId: z.string().optional(),
  delegationId: z.string().optional(),
});

/**
 * Creates the audit trail router.
 */
export const createAuditRouter = (): Router => {
  const router = Router();
  const auditService = AuditService.getInstance();

  /**
   * Story 11.5 / 17.7: GET /api/audit - List audit entries with filtering and pagination.
   * Supports: actorId, eventType, from, to, principalId, delegationId filters.
   * Requires admin role.
   */
  router.get('/', requireRole(['admin']), async (req, res) => {
    try {
      const { actorId, eventType, from, to, limit, offset, principalId, delegationId } =
        AuditQuerySchema.parse(req.query);

      // Delegation-specific filters use raw SQL via pool; otherwise use AuditService
      if (principalId || delegationId) {
        const { pool } = await import('../lib/database.js');
        const qb = new QueryBuilder();

        if (principalId) {
          qb.addCondition("acting_context->>'principal'->>'id' = ?", principalId);
        }
        if (delegationId) {
          qb.addCondition("acting_context->>'delegationTokenId' = ?", delegationId);
        }

        const whereClause = qb.buildWhere();
        const params = qb.getParams();

        const countRes = await pool.query(`SELECT COUNT(*) FROM audit_trail${whereClause}`, params);
        const total = parseInt(countRes.rows[0].count, 10);

        const limitPlaceholder = qb.addParam(limit);
        const offsetPlaceholder = qb.addParam(offset);

        const entriesRes = await pool.query(
          `SELECT * FROM audit_trail${whereClause} ORDER BY sequence DESC LIMIT ${limitPlaceholder} OFFSET ${offsetPlaceholder}`,
          qb.getParams()
        );

        return res.json({ entries: entriesRes.rows, total });
      }

      const result = await auditService.getEntries({
        actorId,
        eventType,
        from,
        to,
        limit,
        offset,
      });

      res.json(result);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  /**
   * Story 11.5: GET /api/audit/export - Stream the full audit trail as JSONL.
   * Requires admin role.
   */
  router.get('/export', requireRole(['admin']), async (req, res) => {
    try {
      const format = req.query.format || 'jsonl';
      if (format !== 'jsonl') {
        return res
          .status(400)
          .json({ error: 'Only jsonl format is supported for streaming export' });
      }

      res.setHeader('Content-Type', 'application/x-jsonlines');
      res.setHeader('Content-Disposition', 'attachment; filename="audit-trail.jsonl"');

      await auditService.streamEntries((row) => {
        res.write(JSON.stringify(row) + '\n');
      });

      res.end();

      // Record the export action itself
      await auditService.record({
        actorType: 'operator',
        actorId: req.operator?.sub || 'unknown',
        actingContext: null,
        eventType: 'audit.exported',
        payload: { format },
      });
    } catch (err: unknown) {
      if (!res.headersSent) {
        res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
      } else {
        res.end();
      }
    }
  });

  /**
   * Story 11.5: GET /api/audit/verify - Verify audit chain integrity.
   * Requires admin role.
   */
  router.get('/verify', requireRole(['admin']), async (req, res) => {
    try {
      const count = req.query.count ? parseInt(req.query.count as string, 10) : undefined;
      const result = await auditService.verifyIntegrity(count);
      res.json(result);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  return router;
};
