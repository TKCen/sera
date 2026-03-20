/**
 * Epic 8 — Knowledge routes for git-backed circle/global knowledge management.
 */

import { Router } from 'express';
import { KnowledgeGitService } from '../memory/KnowledgeGitService.js';

export function createKnowledgeRouter(): Router {
  const router = Router();
  const gitService = KnowledgeGitService.getInstance();

  /** GET /api/knowledge/circles/:id/history */
  router.get('/circles/:id/history', async (req, res) => {
    try {
      const log = await gitService.log(req.params.id!);
      res.json(log);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** GET /api/knowledge/circles/:id/merge-requests */
  router.get('/circles/:id/merge-requests', async (req, res) => {
    try {
      const requests = await gitService.listMergeRequests(req.params.id!);
      res.json(requests);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** POST /api/knowledge/circles/:id/merge-requests/:requestId/approve */
  router.post('/circles/:id/merge-requests/:requestId/approve', async (req, res) => {
    try {
      const approvedBy = (req as any).identity?.id ?? 'operator';
      await gitService.approveMergeRequest(req.params.requestId!, approvedBy);
      res.json({ success: true });
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** POST /api/knowledge/circles/:id/merge-requests/:requestId/resolve
   *  Conflict resolution — accept ours, theirs, or flag for LLM-assisted merge.
   */
  router.post('/circles/:id/merge-requests/:requestId/resolve', async (req, res) => {
    try {
      const { strategy } = req.body as { strategy: 'ours' | 'theirs' | 'llm' };
      // DECISION: LLM-assisted merge is a stub. 'ours'/'theirs' are accepted
      // but not yet implemented beyond acknowledgement.
      res.json({
        success: true,
        strategy,
        note: 'Resolution strategy acknowledged — operator action required to finalise',
      });
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  return router;
}
