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

// ── Router factory ────────────────────────────────────────────────────────────

export function createProvidersRouter(
  llmRouter: LlmRouter,
  circuitBreakerService: CircuitBreakerService
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
    } catch (err: any) {
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
    } catch (err: any) {
      logger.error(`Failed to add provider ${modelName}:`, err);
      res.status(502).json({ error: `Failed to add provider: ${err.message}` });
    }
  });

  /**
   * DELETE /api/providers/:modelName
   * Removes a model from LiteLLM's live configuration.
   * Requires operator role.
   */
  router.delete(
    '/:modelName',
    requireRole(['admin', 'operator']),
    async (req: Request, res: Response) => {
      const modelName = String(req.params['modelName']);
      try {
        await llmRouter.deleteModel(modelName);
        logger.info(
          `Provider removed | model=${modelName} by operator=${req.operator?.sub ?? 'unknown'}`
        );
        res.status(204).end();
      } catch (err: any) {
        logger.error(`Failed to remove provider ${modelName}:`, err);
        res.status(502).json({ error: `Failed to remove provider: ${err.message}` });
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
      res.status(result.ok ? 200 : 502).json(result);
    } catch (err: any) {
      res.status(502).json({ ok: false, error: err.message });
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
