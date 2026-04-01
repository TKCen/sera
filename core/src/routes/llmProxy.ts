/**
 * LLM Proxy Routes — OpenAI-compatible gateway for agent containers.
 *
 * All LLM calls from agents are routed through here. Core enforces:
 *   1. JWT authentication (scope: 'agent' | 'internal')
 *   2. Per-agent token budget (checked BEFORE the upstream call)
 *   3. Per-provider circuit breaking (via opossum)
 *   4. Token usage metering (recorded AFTER the response)
 *   5. Rate limiting hook (stub — see Story 4.7)
 *
 * Endpoints:
 *   POST /v1/llm/chat/completions — proxied chat completion (streaming + non-streaming)
 *   GET  /v1/llm/models           — list available models from LiteLLM
 *
 * @see docs/epics/04-llm-proxy-and-governance.md
 */

import { Router } from 'express';
import type { Request, Response } from 'express';
import type { IdentityService } from '../auth/index.js';
import { createAuthMiddleware } from '../auth/index.js';
import type { AuthService } from '../auth/index.js';
import type { MeteringService } from '../metering/MeteringService.js';
import { rateLimitStub } from '../middleware/rateLimitStub.js';
import type { LlmRouter } from '../llm/index.js';
import type { CircuitBreakerService } from '../llm/index.js';
import { Logger } from '../lib/logger.js';
import type { Pool } from 'pg';
import type { Orchestrator } from '../agents/index.js';
import { ContextAssembler } from '../llm/index.js';
import type { ContextCompactionService } from '../llm/index.js';

const logger = new Logger('LLMProxy');

// ── Scope guard — agent-facing proxy endpoints ────────────────────────────────

/**
 * Middleware that rejects tokens whose scope is not 'agent' or 'internal'.
 * Must run after the JWT auth middleware has populated req.agentIdentity.
 *
 * Story 4.2: Reject anything with scope other than 'agent' on agent-facing proxy endpoints.
 */
function requireAgentScope(req: Request, res: Response, next: () => void): void {
  const identity = req.agentIdentity;
  if (!identity) {
    // Operator tokens (req.operator) are also allowed on the proxy (internal use)
    if (req.operator) {
      next();
      return;
    }
    res.status(401).json({ error: 'Authentication required' });
    return;
  }
  if (identity.scope !== 'agent' && identity.scope !== 'internal') {
    res.status(403).json({
      error: `Invalid token scope '${identity.scope}' — only 'agent' and 'internal' scopes are permitted on the LLM proxy`,
    });
    return;
  }
  next();
}

// ── Router factory ────────────────────────────────────────────────────────────

export function createLlmProxyRouter(
  identityService: IdentityService,
  authService: AuthService,
  meteringService: MeteringService,
  llmRouter: LlmRouter,
  circuitBreakerService: CircuitBreakerService,
  pool: Pool,
  orchestrator: Orchestrator,
  contextCompactionService?: ContextCompactionService
): Router {
  const router = Router();
  const authMiddleware = createAuthMiddleware(identityService, authService);
  const contextAssembler = new ContextAssembler(pool, orchestrator);

  // ── POST /chat/completions ─────────────────────────────────────────────────

  router.post(
    '/chat/completions',
    authMiddleware,
    requireAgentScope,
    rateLimitStub,
    async (req: Request, res: Response) => {
      const latencyStart = Date.now();
      const identity = req.agentIdentity!;
      const agentId = identity?.agentId ?? req.operator?.sub ?? 'unknown';
      const circleId = identity?.circleId ?? null;

      // ── 1. Budget gate (Story 4.3) ─────────────────────────────────────────
      // INVARIANT: No upstream call must be made if budget is exceeded.
      let budget;
      try {
        budget = await meteringService.checkBudget(agentId);
      } catch (err: unknown) {
        logger.error('Budget check failed (allowing request):', err);
        // Fail-open: if metering DB is down, allow the request but log it
        budget = null;
      }

      if (budget && !budget.allowed) {
        const period = budget.hourlyUsed >= budget.hourlyQuota ? 'hourly' : 'daily';
        logger.warn(
          `Budget exceeded | agent=${agentId} ` +
            `hourly=${budget.hourlyUsed}/${budget.hourlyQuota} ` +
            `daily=${budget.dailyUsed}/${budget.dailyQuota}`
        );
        res.status(429).json({
          error: 'budget_exceeded',
          period,
          limit: period === 'hourly' ? budget.hourlyQuota : budget.dailyQuota,
          used: period === 'hourly' ? budget.hourlyUsed : budget.dailyUsed,
        });
        return;
      }

      // ── 2. Validate request body ───────────────────────────────────────────
      const body = req.body as Record<string, unknown>;
      const { model, temperature, tools, stream, thinking_level: thinkingLevel } = body;
      let messages = body['messages'] as import('../agents/index.js').ChatMessage[] | undefined;

      if (!messages || !Array.isArray(messages) || messages.length === 0) {
        res
          .status(400)
          .json({ error: { message: '`messages` array is required and must be non-empty' } });
        return;
      }

      // ── 2.1 Context Assembly (Story 6.3 / 8.4 / #308) ────────────────────
      try {
        if (messages) {
          messages = (await contextAssembler.assemble(
            agentId,
            messages as unknown as import('../llm/index.js').ChatMessage[],
            (event) => {
              logger.info(`[context-assembly] ${event.stage}`, {
                ...event.detail,
                ...(event.durationMs !== undefined ? { durationMs: event.durationMs } : {}),
              });
            }
          )) as unknown as import('../agents/index.js').ChatMessage[];
        }
      } catch (err) {
        logger.error('Context assembly failed (continuing without enrichment):', err);
      }

      const modelName =
        typeof model === 'string' ? model : (process.env.LLM_MODEL ?? 'lmstudio-default');

      // ── 2.2 Context Compaction (#387) ──────────────────────────────────────
      try {
        if (contextCompactionService && messages) {
          messages = (await contextCompactionService.compact(
            messages as unknown as import('../llm/index.js').ChatMessage[],
            modelName,
            (event) => {
              logger.info(`[context-compaction] ${event.stage}`, {
                ...event.detail,
                ...(event.durationMs !== undefined ? { durationMs: event.durationMs } : {}),
              });
            }
          )) as unknown as import('../agents/index.js').ChatMessage[];
        }
      } catch (err) {
        logger.error('Context compaction failed (continuing with full context):', err);
      }

      const chatRequest = {
        model: modelName,
        messages: messages as unknown as import('../llm/index.js').ChatMessage[],
        ...(temperature !== undefined ? { temperature: temperature as number } : {}),
        ...(Array.isArray(tools) ? { tools: tools as unknown[] } : {}),
        ...(thinkingLevel ? { thinkingLevel: thinkingLevel as string } : {}),
      };

      // ── 3. Streaming path ──────────────────────────────────────────────────
      if (stream === true) {
        try {
          logger.info(`Proxy stream | agent=${agentId} model=${modelName}`);
          const streamRes = await llmRouter.chatCompletionStream(
            { ...chatRequest, stream: true },
            agentId
          );

          res.setHeader('Content-Type', 'text/event-stream');
          res.setHeader('Cache-Control', 'no-cache');
          res.setHeader('Connection', 'keep-alive');
          res.setHeader('X-Accel-Buffering', 'no');

          streamRes.pipe(res);
          streamRes.on('end', () => res.end());
          return;
        } catch (err: unknown) {
          const streamErr = err as { code?: string; provider?: string; message: string };
          if (streamErr.code === 'CIRCUIT_OPEN') {
            res.status(503).json({
              error: 'provider_unavailable',
              provider: streamErr.provider,
              message: streamErr.message,
            });
            return;
          }
          logger.error(`Stream proxy error | agent=${agentId}:`, err);
          res.status(502).json({
            error: { message: `Upstream LLM error: ${streamErr.message}`, type: 'upstream_error' },
          });
          return;
        }
      }

      // ── 4. Non-streaming path — call through circuit breaker ───────────────
      logger.info(
        `Proxy request | agent=${agentId} model=${modelName} messages=${messages?.length ?? 0} ` +
          `tools=${Array.isArray(tools) ? tools.length : 0}`
      );

      let llmResponse: Awaited<ReturnType<CircuitBreakerService['call']>> | null = null;
      let callStatus: 'success' | 'error' = 'success';

      try {
        llmResponse = await circuitBreakerService.call(chatRequest, agentId, latencyStart);
      } catch (err: unknown) {
        callStatus = 'error';
        const cbErr = err as { code?: string; provider?: string; message: string };

        if (cbErr.code === 'CIRCUIT_OPEN') {
          res.status(503).json({
            error: 'provider_unavailable',
            provider: cbErr.provider,
            message: cbErr.message,
          });
          return;
        }

        // Record failed call in metering (Story 4.4)
        meteringService
          .recordUsage({
            agentId,
            circleId,
            model: modelName,
            promptTokens: 0,
            completionTokens: 0,
            totalTokens: 0,
            latencyMs: Date.now() - latencyStart,
            status: 'error',
          })
          .catch((merr) => logger.error('Failed to record error metering:', merr));

        logger.error(`LLM proxy error | agent=${agentId}:`, err);
        res.status(502).json({
          error: { message: `Upstream LLM error: ${cbErr.message}`, type: 'upstream_error' },
        });
        return;
      }
      // ── 5. Record metering async (Story 4.4) ──────────────────────────────
      // Non-blocking — does not add latency to the response path.
      if (llmResponse) {
        const usage = llmResponse.response.usage;
        meteringService
          .recordUsage({
            agentId,
            circleId,
            model: modelName,
            promptTokens: usage?.prompt_tokens ?? 0,
            completionTokens: usage?.completion_tokens ?? 0,
            totalTokens: usage?.total_tokens ?? 0,
            latencyMs: llmResponse.latencyMs,
            status: callStatus,
          })
          .catch((err) => logger.error('Failed to record metering:', err));

        // ── 6. Return response ─────────────────────────────────────────────────
        logger.debug(
          `Proxy complete | agent=${agentId} model=${modelName} ` +
            `tokens=${usage?.total_tokens ?? 0} latency=${llmResponse.latencyMs}ms`
        );

        res.json(llmResponse.response);
      }
    }
  );

  // ── GET /models ────────────────────────────────────────────────────────────

  router.get('/models', authMiddleware, async (_req: Request, res: Response) => {
    try {
      const models = await llmRouter.listModels();
      res.json({ object: 'list', data: models });
    } catch (err: unknown) {
      logger.error('Failed to list models:', err);
      res.status(502).json({ error: 'Failed to retrieve model list' });
    }
  });

  return router;
}
