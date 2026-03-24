/**
 * Provider management routes — operator-facing API for LLM provider config.
 *
 * All routes proxy to LiteLLM's model management API, keeping LiteLLM as
 * an implementation detail that operators never interact with directly.
 *
 * Routing strategy and fallback chain changes still require a LiteLLM
 * container restart — these are infrastructure-level settings (not runtime).
 *
 * Endpoints:
 *   GET    /api/providers                       — list providers
 *   POST   /api/providers                       — add provider (operator only)
 *   DELETE /api/providers/:modelName            — remove provider (operator only)
 *   POST   /api/providers/:modelName/test       — test provider connectivity
 *   GET    /api/providers/:modelName/health     — circuit breaker state
 *   GET    /api/system/circuit-breakers         — all circuit breaker states
 *
 * @see docs/epics/04-llm-proxy-and-governance.md § Story 4.5, 4.6
 */

import { Router } from 'express';
import type { Request, Response } from 'express';
import { z } from 'zod';
import type { LlmRouter } from '../llm/LlmRouter.js';
import type { CircuitBreakerService } from '../llm/CircuitBreakerService.js';
import { providerFromModel } from '../llm/CircuitBreakerService.js';
import { requireRole } from '../auth/authMiddleware.js';
import { Logger } from '../lib/logger.js';
import type { DynamicProviderManager } from '../llm/DynamicProviderManager.js';
import { ProviderHealthService } from '../llm/ProviderHealthService.js';

const logger = new Logger('ProvidersRoute');

// ── Validation schema ─────────────────────────────────────────────────────────

const AddProviderSchema = z.object({
  modelName: z.string().min(1),
  api: z.enum(['openai-completions', 'anthropic-messages']).default('openai-completions'),
  provider: z.string().optional(),
  baseUrl: z.string().url().optional(),
  /** Literal API key — prefer apiKeyEnvVar. */
  apiKey: z.string().optional(),
  /** Name of the env var that holds the API key (read at request time). */
  apiKeyEnvVar: z.string().optional(),
  description: z.string().optional(),
});

const AddDynamicProviderSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  type: z.literal('lm-studio'),
  baseUrl: z.string().url(),
  apiKey: z.string().optional(),
  enabled: z.boolean().default(true),
  intervalMs: z.number().min(5000).default(60000),
  description: z.string().optional(),
});

// ── Router factory ────────────────────────────────────────────────────────────

export function createProvidersRouter(
  llmRouter: LlmRouter,
  circuitBreakerService: CircuitBreakerService,
  dynamicProviderManager: DynamicProviderManager
): Router {
  const router = Router();
  const healthService = new ProviderHealthService();

  /**
   * GET /api/providers/templates
   * Returns available cloud provider templates that can be activated.
   */
  router.get('/templates', (_req: Request, res: Response) => {
    res.json({
      templates: [
        {
          provider: 'openai',
          displayName: 'OpenAI',
          api: 'openai-completions',
          models: ['gpt-4.1', 'gpt-4.1-mini', 'gpt-4.1-nano', 'o4-mini', 'o3-pro'],
          apiKeyEnvVar: 'OPENAI_API_KEY',
          description: 'OpenAI GPT and reasoning models',
        },
        {
          provider: 'anthropic',
          displayName: 'Anthropic',
          api: 'anthropic-messages',
          models: ['claude-opus-4-6', 'claude-sonnet-4-6', 'claude-haiku-4-5'],
          apiKeyEnvVar: 'ANTHROPIC_API_KEY',
          description: 'Anthropic Claude models',
        },
        {
          provider: 'google',
          displayName: 'Google AI Studio',
          api: 'openai-completions',
          models: ['gemini-2.5-pro', 'gemini-2.5-flash'],
          baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai',
          apiKeyEnvVar: 'GOOGLE_API_KEY',
          description: 'Google Gemini models via AI Studio (free tier available)',
          supportsDiscovery: true,
        },
        {
          provider: 'groq',
          displayName: 'Groq',
          api: 'openai-completions',
          models: ['groq/llama-4-scout-17b', 'groq/llama-4-maverick-17b'],
          baseUrl: 'https://api.groq.com/openai/v1',
          apiKeyEnvVar: 'GROQ_API_KEY',
          description: 'Groq inference (fast, free tier available)',
        },
        {
          provider: 'mistral',
          displayName: 'Mistral',
          api: 'openai-completions',
          models: ['mistral-large-latest', 'mistral-small-latest'],
          baseUrl: 'https://api.mistral.ai/v1',
          apiKeyEnvVar: 'MISTRAL_API_KEY',
          description: 'Mistral AI models',
        },
      ],
    });
  });

  /**
   * GET /api/providers/list
   * Lists all models/providers currently configured.
   * Note: Express 5 doesn't match router.get('/') for mounted sub-routers.
   */
  router.get('/list', async (_req: Request, res: Response) => {
    try {
      const models = await llmRouter.listModels();
      res.json({ providers: models });
    } catch (err: unknown) {
      logger.error('Failed to list providers:', err);
      res.status(502).json({ error: 'Failed to retrieve provider list' });
    }
  });

  /**
   * POST /api/providers
   * Adds a new model/provider to LiteLLM's live configuration.
   * Requires operator role.
   *
   * Note: Adding a new model is hot-reloadable (no LiteLLM restart needed).
   * Routing strategy and fallback chain changes require a restart.
   */
  router.post('/', requireRole(['admin', 'operator']), async (req: Request, res: Response) => {
    const parsed = AddProviderSchema.safeParse(req.body);
    if (!parsed.success) {
      res.status(400).json({ error: 'Invalid provider config', details: parsed.error.flatten() });
      return;
    }

    const { modelName, api, provider, baseUrl, apiKey, apiKeyEnvVar, description } = parsed.data;

    try {
      // If no API key provided, try to inherit from an existing model of the same provider
      let resolvedApiKey = apiKey;
      let resolvedBaseUrl = baseUrl;
      if (!resolvedApiKey && provider) {
        const existing = llmRouter.getRegistry().list().find(
          (c) => c.provider === provider && c.apiKey
        );
        if (existing) {
          resolvedApiKey = existing.apiKey;
          if (!resolvedBaseUrl && existing.baseUrl) resolvedBaseUrl = existing.baseUrl;
        }
      }

      const result = await llmRouter.addModel({
        modelName,
        api,
        ...(provider ? { provider } : {}),
        ...(resolvedBaseUrl ? { baseUrl: resolvedBaseUrl } : {}),
        ...(resolvedApiKey ? { apiKey: resolvedApiKey } : {}),
        ...(apiKeyEnvVar ? { apiKeyEnvVar } : {}),
        ...(description ? { description } : {}),
      });
      logger.info(
        `Provider added | model=${modelName} by operator=${req.operator?.sub ?? 'unknown'}`
      );
      res.status(201).json({ modelName, result });
    } catch (err: unknown) {
      logger.error(`Failed to add provider ${modelName}:`, err);
      res.status(502).json({ error: `Failed to add provider: ${(err as Error).message}` });
    }
  });

  // ── Dynamic Providers ──────────────────────────────────────────────────────
  // These specific subpaths must be defined BEFORE the parameterized /:modelName routes
  // to avoid being shadowed.

  /**
   * GET /api/providers/dynamic
   * Lists all configured dynamic providers.
   */
  router.get('/dynamic', requireRole(['admin', 'operator']), (_req: Request, res: Response) => {
    res.json({ dynamicProviders: dynamicProviderManager.listProviders() });
  });

  /**
   * GET /api/providers/dynamic/statuses
   * Returns the last check status for all dynamic providers.
   */
  router.get(
    '/dynamic/statuses',
    requireRole(['admin', 'operator']),
    (_req: Request, res: Response) => {
      res.json({ statuses: dynamicProviderManager.getStatuses() });
    }
  );

  /**
   * POST /api/providers/dynamic
   * Adds or updates a dynamic provider configuration.
   */
  router.post(
    '/dynamic',
    requireRole(['admin', 'operator']),
    async (req: Request, res: Response) => {
      const parsed = AddDynamicProviderSchema.safeParse(req.body);
      if (!parsed.success) {
        res
          .status(400)
          .json({ error: 'Invalid dynamic provider config', details: parsed.error.flatten() });
        return;
      }

      try {
        await dynamicProviderManager.addProvider(parsed.data);
        res.status(201).json(parsed.data);
      } catch (err: unknown) {
        logger.error('Failed to add dynamic provider:', err);
        res.status(502).json({ error: (err as Error).message });
      }
    }
  );

  /**
   * DELETE /api/providers/dynamic/:id
   * Removes a dynamic provider and its models.
   */
  router.delete(
    '/dynamic/:id',
    requireRole(['admin', 'operator']),
    async (req: Request, res: Response) => {
      const id = String(req.params['id']);
      try {
        await dynamicProviderManager.removeProvider(id);
        res.status(204).end();
      } catch (err: unknown) {
        logger.error(`Failed to remove dynamic provider ${id}:`, err);
        res.status(502).json({ error: (err as Error).message });
      }
    }
  );

  /**
   * POST /api/providers/dynamic/test
   * Tests a connection to a dynamic provider URL.
   */
  router.post(
    '/dynamic/test',
    requireRole(['admin', 'operator']),
    async (req: Request, res: Response) => {
      const { baseUrl, apiKey } = req.body;
      if (!baseUrl) {
        res.status(400).json({ error: 'baseUrl is required' });
        return;
      }

      try {
        const result = await dynamicProviderManager.testConnection(baseUrl, apiKey);
        res.json(result);
      } catch (err: unknown) {
        res.status(502).json({ success: false, error: (err as Error).message });
      }
    }
  );

  // ── Static Providers ────────────────────────────────────────────────────────

  /**
   * DELETE /api/providers/:modelName
   * Removes a model from the active provider registry and persists the change.
   * For dynamic-provider models, delegates to DynamicProviderManager.
   */
  router.delete(
    '/:modelName',
    requireRole(['admin', 'operator']),
    async (req: Request, res: Response) => {
      const modelName = String(req.params['modelName']);
      try {
        await llmRouter.deleteModel(modelName);
        logger.info(
          `Provider deleted | model=${modelName} by operator=${req.operator?.sub ?? 'unknown'}`
        );
        res.status(204).end();
      } catch (err: unknown) {
        const msg = (err as Error).message;
        const code =
          msg.includes('not found') || msg.includes('No provider registered') ? 404 : 500;
        res.status(code).json({ error: msg });
      }
    }
  );

  /**
   * POST /api/providers/:modelName/test
   * Sends a minimal test completion to verify the provider is reachable.
   */
  router.post('/:modelName/test', async (req: Request, res: Response) => {
    const modelName = String(req.params['modelName']);
    try {
      const result = await llmRouter.testModel(modelName);
      res.status(result.ok ? 200 : 502).json({
        success: result.ok,
        latencyMs: result.latencyMs,
        ...(result.error !== undefined ? { error: result.error } : {}),
      });
    } catch (err: unknown) {
      res.status(502).json({ success: false, error: (err as Error).message });
    }
  });

  /**
   * GET /api/providers/:modelName/health
   * Returns the circuit breaker state for the provider associated with this model.
   */
  router.get('/:modelName/health', (req: Request, res: Response) => {
    const modelName = String(req.params['modelName']);
    const provider = providerFromModel(modelName);
    const state = circuitBreakerService.getProviderState(provider);

    if (!state) {
      // No breaker exists yet — provider has never been called through Core
      res.json({
        provider,
        state: 'closed',
        message: 'No circuit breaker instantiated (no calls made yet)',
      });
      return;
    }

    res.json(state);
  });

  // ── Default Model ──────────────────────────────────────────────────────────

  /** GET /api/providers/default-model — get the current default model name. */
  router.get('/default-model', (_req: Request, res: Response) => {
    const registry = llmRouter.getRegistry();
    res.json({ defaultModel: registry.getDefaultModel() });
  });

  /** PUT /api/providers/default-model — set the default model name. */
  router.put('/default-model', (req: Request, res: Response) => {
    try {
      const { modelName } = req.body as { modelName: string };
      if (!modelName) {
        return res.status(400).json({ error: 'modelName is required' });
      }
      const registry = llmRouter.getRegistry();
      registry.setDefaultModel(modelName);
      res.json({ success: true, defaultModel: modelName });
    } catch (err: unknown) {
      res.status(400).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  /**
   * GET /api/providers/health-all
   * Batch health check all providers (cached 60s).
   */
  router.get('/health-all', async (_req: Request, res: Response) => {
    try {
      const registry = llmRouter.getRegistry();
      const configs = registry.list();
      // Deduplicate by provider+baseUrl to avoid redundant probes
      const seen = new Set<string>();
      const results: Record<string, unknown> = {};

      for (const cfg of configs) {
        const key = `${cfg.provider ?? 'unknown'}:${cfg.baseUrl ?? 'cloud'}`;
        if (seen.has(key)) continue;
        seen.add(key);

        const status = await healthService.checkHealth(cfg);
        results[cfg.modelName] = {
          provider: cfg.provider,
          ...status,
        };
      }

      res.json({ health: results });
    } catch (err: unknown) {
      logger.error('Health check failed:', err);
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * GET /api/providers/:modelName/discover
   * Discover models available at the provider's endpoint.
   */
  router.get('/:modelName/discover', async (req: Request, res: Response) => {
    try {
      const registry = llmRouter.getRegistry();
      const config = registry.list().find((c) => c.modelName === req.params.modelName);
      if (!config) {
        return res.status(404).json({ error: `Provider '${req.params.modelName}' not found` });
      }

      const models = await healthService.discoverModels(config);
      res.json({ provider: config.modelName, models });
    } catch (err: unknown) {
      logger.error('Model discovery failed:', err);
      res.status(500).json({ error: (err as Error).message });
    }
  });

  return router;
}

/**
 * System-level circuit breaker status endpoint.
 * Mounted at GET /api/system/circuit-breakers.
 */
export function createSystemRouter(circuitBreakerService: CircuitBreakerService): Router {
  const router = Router();

  router.get('/circuit-breakers', (_req: Request, res: Response) => {
    res.json({ circuitBreakers: circuitBreakerService.getState() });
  });

  return router;
}
