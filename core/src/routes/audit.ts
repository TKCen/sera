import { Router } from 'express';
import { AuditService } from '../audit/AuditService.js';
import { requireRole } from '../auth/authMiddleware.js';

/**
 * Creates the audit trail router.
 */
export const createAuditRouter = () => {
  const router = Router();
  const auditService = AuditService.getInstance();

  /**
   * Story 11.5: GET /api/audit - List audit entries with filtering and pagination.
   * Requires admin role.
   */
  router.get('/', requireRole(['admin']), async (req, res) => {
    try {
      const { actorId, eventType, from, to, limit, offset } = req.query;
      
      const result = await auditService.getEntries({
        actorId: actorId as string,
        eventType: eventType as string,
        from: from as string,
        to: to as string,
        limit: limit ? parseInt(limit as string, 10) : 50,
        offset: offset ? parseInt(offset as string, 10) : 0,
      });
      
      res.json(result);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
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
        return res.status(400).json({ error: 'Only jsonl format is supported for streaming export' });
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
        payload: { format }
      });

    } catch (err: any) {
      if (!res.headersSent) {
        res.status(500).json({ error: err.message });
      } else {
        res.end();
      }
    }
  });

  return router;
};
