import { Router } from 'express';
import { requireRole } from '../auth/authMiddleware.js';
import { ApiKeyService } from '../auth/api-key-service.js';

export function createAuthRouter() {
  const router = Router();

  /**
   * GET /api/auth/me
   * Returns the authenticated operator's identity and roles.
   */
  router.get('/me', (req, res) => {
    if (!req.operator) {
      res.status(401).json({ error: 'Not authenticated as operator' });
      return;
    }

    res.json(req.operator);
  });

  // API Key management (Story 16.3)
  router.get('/api-keys', async (req, res) => {
    try {
      const keys = await ApiKeyService.listKeys(req.operator!.sub);
      res.json(keys);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  router.post('/api-keys', async (req, res) => {
    try {
      const { name, roles, expiresInDays } = req.body;
      if (!name) {
        res.status(400).json({ error: 'Name is required' });
        return;
      }

      // Default to ['viewer'] if not provided, and validate roles if necessary
      const keyRoles = roles || ['viewer'];

      const result = await ApiKeyService.createKey({
        name,
        ownerSub: req.operator!.sub,
        roles: keyRoles,
        expiresInDays,
      });

      res.status(201).json(result);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  router.delete('/api-keys/:id', async (req, res) => {
    try {
      const revoked = await ApiKeyService.revokeKey(req.params.id, req.operator!.sub);
      if (!revoked) {
        res.status(404).json({ error: 'API key not found or already revoked' });
        return;
      }
      res.json({ message: 'API key revoked' });
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  return router;
}
