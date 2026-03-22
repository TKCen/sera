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

  /**
   * GET /api/providers
   * Lists all models/providers currently configured in LiteLLM.
   */
  router.get('/', async (_req: Request, res: Response) => {
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
      const result = await llmRouter.addModel({
        modelName,
        api,
        ...(provider ? { provider } : {}),
        ...(baseUrl ? { baseUrl } : {}),
        ...(apiKey ? { apiKey } : {}),
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
