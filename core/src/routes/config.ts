import { Router } from 'express';
import type { Request, Response } from 'express';
import { config } from '../lib/config.js';
import { ProviderFactory } from '../lib/llm/ProviderFactory.js';
import { PROVIDER_CATALOG } from '../lib/providers.js';
import { Logger } from '../lib/logger.js';
import { OpenAIProvider } from '../lib/llm/OpenAIProvider.js';

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
    } catch (err: any) {
      logger.error('Failed to update LLM config:', err);
      res.status(500).json({ error: err.message });
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
        response: response.content
      });
    } catch (err: any) {
      logger.error('LLM test failed:', err);
      res.json({ success: false, error: err.message });
    }
  });

  // ─── Provider Management ────────────────────────────────────────────────────

  /** GET /api/providers — Lists all available provider configurations. */
  router.get('/providers', (req: Request, res: Response) => {
    const providersConfig = config.providers;
    const providers = PROVIDER_CATALOG.map(p => {
      const savedConfig = providersConfig.providers[p.id] || null;
      return {
        ...p,
        configured: !!savedConfig,
        isActive: providersConfig.activeProvider === p.id,
        savedConfig
      };
    });
    res.json({
      activeProvider: providersConfig.activeProvider,
      providers
    });
  });

  /** PUT /api/providers/:id — Updates settings for a specific provider. */
  router.put('/providers/:id', (req: Request, res: Response) => {
    const id = String(req.params.id);
    try {
      config.saveProviderConfig(id, req.body);
      logger.info(`Provider configuration updated: ${id}`);
      res.json({ success: true });
    } catch (err: any) {
      logger.error(`Failed to update provider config (${id}):`, err);
      res.status(500).json({ error: err.message });
    }
  });

  /** POST /api/providers/:id/test — Validates a specific provider connection. */
  router.post('/providers/:id/test', async (req: Request, res: Response) => {
    const id = String(req.params.id);
    try {
      const baseUrl = req.body.baseUrl ? String(req.body.baseUrl) : undefined;
      const apiKey = req.body.apiKey ? String(req.body.apiKey) : undefined;
      const model = req.body.model ? String(req.body.model) : undefined;

      let provider;
      if (baseUrl) {
        // If parameters are passed in the body, test them directly
        provider = new OpenAIProvider({
          baseUrl,
          apiKey: apiKey || 'not-needed',
          model: model || 'model-identifier',
        });
      } else {
        // Otherwise, use saved or catalog defaults
        const providerConfig = config.getProviderConfig(id);
        const catalogEntry = PROVIDER_CATALOG.find(p => p.id === id);

        provider = ProviderFactory.createFromModelConfig({
          provider: id,
          name: providerConfig?.model || catalogEntry?.models[0]?.id || 'model-identifier',
        });
      }

      const response = await provider.chat([{ role: 'user', content: 'Hello' }]);
      res.json({
        success: true,
        provider: id,
        response: response.content
      });
    } catch (err: any) {
      // In tests, we might want to see the error, but we should not crash the test suite.
      // The frontend expects success: false and the error message.
      res.json({ success: false, error: err.message });
    }
  });

  /** POST /api/providers/active — Sets a specific provider as the globally active LLM provider. */
  router.post('/providers/active', (req: Request, res: Response) => {
    const providerId = req.body.providerId ? String(req.body.providerId) : undefined;
    try {
      if (!providerId) {
        return res.status(400).json({ error: 'providerId is required' });
      }
      config.setActiveProvider(providerId);
      logger.info(`Active provider set to: ${providerId}`);
      res.json({ success: true, activeProvider: providerId });
    } catch (err: any) {
      logger.error(`Failed to set active provider (${providerId}):`, err);
      res.status(500).json({ error: err.message });
    }
  });

  return router;
}
