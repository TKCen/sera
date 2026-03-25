import { Router } from 'express';
import { z } from 'zod';
import { Logger } from '../lib/logger.js';
import { EmbeddingService } from '../services/embedding.service.js';
import { EmbeddingRouter as EmbeddingRouterService } from '../services/embedding-router.js';
import {
  loadEmbeddingConfig,
  saveEmbeddingConfig,
  maskConfig,
  KNOWN_EMBEDDING_MODELS,
} from '../services/embedding-config.js';
import type { EmbeddingConfig } from '../services/embedding-config.js';

const logger = new Logger('EmbeddingRoutes');

const EmbeddingConfigSchema = z.object({
  provider: z.enum(['ollama', 'openai', 'lm-studio', 'openai-compatible']),
  model: z.string().min(1),
  baseUrl: z.string().min(1),
  apiKey: z.string().optional(),
  apiKeyEnvVar: z.string().optional(),
  dimension: z.number().int().min(1).max(8192),
});

export function createEmbeddingRouter(embeddingService: EmbeddingService): Router {
  const router = Router();

  /**
   * GET /api/embedding/config — current embedding configuration (API keys masked)
   */
  router.get('/config', (_req, res) => {
    const config = embeddingService.getRouter().getConfig();
    res.json(maskConfig(config));
  });

  /**
   * PUT /api/embedding/config — update embedding configuration
   */
  router.put('/config', async (req, res) => {
    try {
      const raw = EmbeddingConfigSchema.parse(req.body);
      const parsed: EmbeddingConfig = {
        provider: raw.provider,
        model: raw.model,
        baseUrl: raw.baseUrl,
        dimension: raw.dimension,
        ...(raw.apiKey ? { apiKey: raw.apiKey } : {}),
        ...(raw.apiKeyEnvVar ? { apiKeyEnvVar: raw.apiKeyEnvVar } : {}),
      };
      const oldConfig = embeddingService.getRouter().getConfig();
      const dimensionChanged = oldConfig.dimension !== parsed.dimension;

      // Save to file
      saveEmbeddingConfig(parsed);

      // Hot-swap in the running service
      embeddingService.reconfigure(parsed);

      // Verify connectivity
      const test = await embeddingService.getRouter().testConnection();

      const response: Record<string, unknown> = {
        config: maskConfig(parsed),
        testResult: test,
      };
      if (dimensionChanged) {
        response.dimensionChanged = true;
        response.warning =
          `Vector dimension changed from ${oldConfig.dimension} to ${parsed.dimension}. ` +
          `Existing vectors are incompatible and will need to be re-indexed.`;
      }

      res.json(response);
    } catch (err) {
      if (err instanceof z.ZodError) {
        res.status(400).json({ error: 'Invalid config', details: err.errors });
        return;
      }
      logger.error('Failed to update embedding config', err);
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * POST /api/embedding/test — test an embedding config without persisting
   */
  router.post('/test', async (req, res) => {
    try {
      const raw = EmbeddingConfigSchema.parse(req.body);
      const parsed: EmbeddingConfig = {
        provider: raw.provider,
        model: raw.model,
        baseUrl: raw.baseUrl,
        dimension: raw.dimension,
        ...(raw.apiKey ? { apiKey: raw.apiKey } : {}),
        ...(raw.apiKeyEnvVar ? { apiKeyEnvVar: raw.apiKeyEnvVar } : {}),
      };
      const testRouter = new EmbeddingRouterService(parsed);
      const result = await testRouter.testConnection();
      res.json(result);
    } catch (err) {
      if (err instanceof z.ZodError) {
        res.status(400).json({ error: 'Invalid config', details: err.errors });
        return;
      }
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * GET /api/embedding/models — discover available embedding models
   */
  router.get('/models', async (req, res) => {
    try {
      const config = embeddingService.getRouter().getConfig();
      const provider = (req.query.provider as string) ?? config.provider;
      const baseUrl = (req.query.baseUrl as string) ?? config.baseUrl;

      const models: Array<{ id: string; dimension?: number; description?: string }> = [];

      // Always include known models for the provider
      for (const [id, info] of Object.entries(KNOWN_EMBEDDING_MODELS)) {
        if (info.provider === provider || provider === 'openai-compatible') {
          models.push({ id, dimension: info.dimension, description: info.description });
        }
      }

      // Try to discover from the endpoint
      if (baseUrl) {
        try {
          if (provider === 'ollama') {
            const resp = await fetch(`${baseUrl.replace(/\/$/, '')}/api/tags`, {
              signal: AbortSignal.timeout(5_000),
            });
            if (resp.ok) {
              const data = (await resp.json()) as { models?: Array<{ name: string }> };
              for (const m of data.models ?? []) {
                // Only include models that look like embedding models
                if (
                  m.name.includes('embed') ||
                  m.name.includes('minilm') ||
                  m.name.includes('bge')
                ) {
                  if (!models.some((e) => e.id === m.name)) {
                    const known = KNOWN_EMBEDDING_MODELS[m.name];
                    models.push({
                      id: m.name,
                      ...(known?.dimension !== undefined ? { dimension: known.dimension } : {}),
                      description: known?.description ?? 'Discovered from Ollama',
                    });
                  }
                }
              }
            }
          } else {
            // OpenAI-compatible: GET /v1/models
            const headers: Record<string, string> = {};
            const apiKey = config.apiKey ?? process.env.OPENAI_API_KEY;
            if (apiKey) headers['Authorization'] = `Bearer ${apiKey}`;

            const resp = await fetch(`${baseUrl.replace(/\/$/, '')}/v1/models`, {
              headers,
              signal: AbortSignal.timeout(5_000),
            });
            if (resp.ok) {
              const data = (await resp.json()) as { data?: Array<{ id: string }> };
              for (const m of data.data ?? []) {
                if (m.id.includes('embed')) {
                  if (!models.some((e) => e.id === m.id)) {
                    const known = KNOWN_EMBEDDING_MODELS[m.id];
                    models.push({
                      id: m.id,
                      ...(known?.dimension !== undefined ? { dimension: known.dimension } : {}),
                      description: known?.description ?? 'Discovered',
                    });
                  }
                }
              }
            }
          }
        } catch {
          // Discovery failed — still return known models
          logger.debug('Model discovery failed, returning known models only');
        }
      }

      res.json({ models });
    } catch (err) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * GET /api/embedding/status — embedding service status
   */
  router.get('/status', (_req, res) => {
    const config = embeddingService.getRouter().getConfig();
    res.json({
      available: embeddingService.isAvailable(),
      configured: embeddingService.isConfigured(),
      provider: config.provider,
      model: config.model,
      dimension: config.dimension,
      baseUrl: config.baseUrl,
    });
  });

  /**
   * GET /api/embedding/known-models — static list of known embedding models
   */
  router.get('/known-models', (_req, res) => {
    res.json(KNOWN_EMBEDDING_MODELS);
  });

  return router;
}
