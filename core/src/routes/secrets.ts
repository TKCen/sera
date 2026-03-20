import { Router } from 'express';
import { SecretsManager } from '../secrets/secrets-manager.js';
import { requireRole } from '../auth/authMiddleware.js';

export function createSecretsRouter() {
  const router = Router();
  const secrets = SecretsManager.getInstance();

  /**
   * GET /api/secrets
   * List secrets metadata.
   */
  router.get('/', requireRole(['admin', 'operator']), async (req, res) => {
    try {
      const list = await secrets.list(
        {
          // Optional filters could be added here
        },
        {
          operator: req.operator!,
        }
      );
      res.json(list);
    } catch (err: any) {
      res.status(err.message.includes('Unauthorized') ? 403 : 500).json({ error: err.message });
    }
  });

  /**
   * GET /api/secrets/:key
   * Get a secret value.
   */
  router.get('/:key', requireRole(['admin', 'operator']), async (req, res) => {
    try {
      const secret = await secrets.get(req.params.key as string, {
        operator: req.operator!,
      });
      if (!secret) {
        res.status(404).json({ error: 'Secret not found' });
        return;
      }
      res.json(secret);
    } catch (err: any) {
      res.status(err.message.includes('Unauthorized') ? 403 : 500).json({ error: err.message });
    }
  });

  /**
   * POST /api/secrets
   * Create or update a secret.
   */
  router.post('/', requireRole(['admin', 'operator']), async (req, res) => {
    try {
      const { key, value, description, tags, allowedAgents, allowedCircles } = req.body;
      if (!key || value === undefined) {
        res.status(400).json({ error: 'Key and value are required' });
        return;
      }

      await secrets.set(
        key,
        value,
        {
          operator: req.operator!,
        },
        {
          description,
          tags,
          allowedAgents: allowedAgents || [],
          allowedCircles: allowedCircles || [],
          exposure: (req.body.exposure as any) || 'agent-env',
        } as any
      );

      res.status(201).json({ message: 'Secret stored' });
    } catch (err: any) {
      res.status(err.message.includes('Unauthorized') ? 403 : 500).json({ error: err.message });
    }
  });

  /**
   * DELETE /api/secrets/:key
   * Delete a secret.
   */
  router.delete('/:key', requireRole(['admin']), async (req, res) => {
    try {
      const deleted = await secrets.delete(req.params.key as string, {
        operator: req.operator!,
      });
      if (!deleted) {
        res.status(404).json({ error: 'Secret not found' });
        return;
      }
      res.json({ message: 'Secret deleted' });
    } catch (err: any) {
      res.status(err.message.includes('Unauthorized') ? 403 : 500).json({ error: err.message });
    }
  });

  return router;
}
