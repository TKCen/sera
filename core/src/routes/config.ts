import { Router } from 'express';
import type { Request, Response } from 'express';
import { config } from '../lib/config.js';
import { ProviderFactory } from '../lib/llm/ProviderFactory.js';
import { Logger } from '../lib/logger.js';
import { sanitizeErrorMessage } from '../middleware/errorSanitizer.js';

const logger = new Logger('ConfigRouter');

/**
 * Config Router — handles system-wide LLM and Provider configuration.
 *
 * Endpoints:
 *   GET  /config/llm          — get legacy LLM config
 *   POST /config/llm          — update legacy LLM config
 *   POST /config/llm/test     — test legacy LLM connection
 *   GET  /providers           — list available providers & active status
 *   PUT  /providers/:id       — update specific provider config
 *   POST /providers/:id/test  — test specific provider connection
 *   POST /providers/active    — set global active provider
 */
export function createConfigRouter(): Router {
  const router = Router();

  // ─── Legacy LLM Config ──────────────────────────────────────────────────────

  /** GET /api/config/llm — Returns current legacy LLM configuration. */
  router.get('/config/llm', (req: Request, res: Response) => {
    res.json(config.llm);
  });

  /** POST /api/config/llm — Updates the legacy LLM configuration. */
  router.post('/config/llm', (req: Request, res: Response) => {
    try {
      config.saveLlmConfig(req.body);
      logger.info('Legacy LLM configuration updated');
      res.json({ success: true });
    } catch (err: unknown) {
      logger.error('Failed to update LLM config:', err);
      res
        .status(500)
        .json({ error: sanitizeErrorMessage(err instanceof Error ? err.message : String(err)) });
    }
  });

  /** POST /api/config/llm/test — Tests the current LLM connection. */
  router.post('/config/llm/test', async (req: Request, res: Response) => {
    try {
      const provider = ProviderFactory.createDefault();
      const response = await provider.chat([{ role: 'user', content: 'Hello' }]);
      res.json({
        success: true,
        model: config.llm.model,
        response: response.content,
      });
    } catch (err: unknown) {
      logger.error('LLM test failed:', err);
      res.json({
        success: false,
        error: sanitizeErrorMessage(err instanceof Error ? err.message : String(err)),
      });
    }
  });

  // ─── Provider Management ────────────────────────────────────────────────────
  // NOTE: Provider CRUD routes (GET/POST/DELETE /api/providers, templates, health,
  // discover) are in routes/providers.ts via createProvidersRouter.
  // The legacy catalog-based routes below have been removed to avoid conflicts.

  return router;
}
