import { Router } from 'express';
import { AuditService } from '../audit/AuditService.js';
import { requireRole } from '../auth/authMiddleware.js';

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
      const { actorId, eventType, from, to, limit, offset, principalId, delegationId } = req.query;

      // Delegation-specific filters use raw SQL via pool; otherwise use AuditService
      if (principalId || delegationId) {
        const { pool } = await import('../lib/database.js');
        const conditions: string[] = [];
        const params: unknown[] = [];

        if (principalId) {
          params.push(principalId as string);
          conditions.push(`acting_context->>'principal'->>'id' = $${params.length}`);
        }
        if (delegationId) {
          params.push(delegationId as string);
          conditions.push(`acting_context->>'delegationTokenId' = $${params.length}`);
        }

        const whereClause = `WHERE ${conditions.join(' AND ')}`;
        const limitVal = limit ? parseInt(limit as string, 10) : 50;
        const offsetVal = offset ? parseInt(offset as string, 10) : 0;

        const countRes = await pool.query(
          `SELECT COUNT(*) FROM audit_trail ${whereClause}`,
          params
        );
        const total = parseInt(countRes.rows[0].count, 10);

        const entriesRes = await pool.query(
          `SELECT * FROM audit_trail ${whereClause} ORDER BY sequence DESC LIMIT $${params.length + 1} OFFSET $${params.length + 2}`,
          [...params, limitVal, offsetVal]
        );

        return res.json({ entries: entriesRes.rows, total });
      }

      const result = await auditService.getEntries({
        actorId: actorId as string,
        eventType: eventType as string,
        from: from as string,
        to: to as string,
        limit: limit ? parseInt(limit as string, 10) : 50,
        offset: offset ? parseInt(offset as string, 10) : 0,
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
