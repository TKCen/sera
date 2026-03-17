import { Router } from 'express';
import { AuditService } from '../audit/AuditService.js';

/**
 * Creates the audit trail router.
 */
export const createAuditRouter = () => {
  const router = Router();
  const auditService = AuditService.getInstance();

  /**
   * Get audit trail for a specific agent.
   */
  router.get('/:agentId', async (req, res) => {
    try {
      const trail = await auditService.getTrail(req.params.agentId);
      res.json(trail);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /**
   * Verify integrity of an agent's audit trail.
   */
  router.get('/:agentId/verify', async (req, res) => {
    try {
      const result = await auditService.verify(req.params.agentId);
      res.json(result);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  return router;
};
