import { Router } from 'express';

/**
 * Federation API routes — Story 9.6 stub.
 */
export function createFederationRouter(): Router {
  const router = Router();

  /**
   * List known federation peers.
   * Story 9.6: Returns empty list in v1.
   */
  router.get('/peers', (req, res) => {
    res.json([]);
  });

  return router;
}
