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

import { PassThrough } from 'stream';
import { Router } from 'express';
import type { Request, Response } from 'express';
import type { IdentityService } from '../auth/IdentityService.js';
import { createAuthMiddleware } from '../auth/authMiddleware.js';
import type { AuthService } from '../auth/auth-service.js';
import type { MeteringService } from '../metering/MeteringService.js';
import { rateLimitStub } from '../middleware/rateLimitStub.js';
import type { LlmRouter } from '../llm/LlmRouter.js';
import { RateLimitedError } from '../llm/LlmRouter.js';
import type { CircuitBreakerService } from '../llm/CircuitBreakerService.js';
import { providerFromModel } from '../llm/CircuitBreakerService.js';
import { Logger } from '../lib/logger.js';
import type { Pool } from 'pg';
import type { Orchestrator } from '../agents/Orchestrator.js';
import { ContextAssembler } from '../llm/ContextAssembler.js';
import type { ContextCompactionService } from '../llm/ContextCompactionService.js';
import { TraceService } from '../services/TraceService.js';

const logger = new Logger('LLMProxy');
const traceService = TraceService.getInstance();

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

  // Track consecutive metering service failures to implement a fail-closed circuit breaker
  let consecutiveMeteringFailures = 0;

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
      const sessionId = (req.headers['x-sera-session-id'] as string | undefined) ?? agentId;

      // ── 1. Budget gate (Story 4.3) ─────────────────────────────────────────
      // INVARIANT: No upstream call must be made if budget is exceeded.
      let budget;
      try {
        budget = await meteringService.checkBudget(agentId);
        consecutiveMeteringFailures = 0; // Reset on success
      } catch (err: unknown) {
        consecutiveMeteringFailures++;
        logger.error(`Budget check failed (${consecutiveMeteringFailures}/3):`, err);

        // Fail-closed after 3 consecutive failures
        if (consecutiveMeteringFailures >= 3) {
          res.status(503).json({
            error: 'metering_unavailable',
            message:
              'Service temporarily unavailable due to metering system failure. Please try again later.',
          });
          return;
        }

        // Fail-open for the first 2 failures: if metering DB is down, allow the request but log it
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
      let messages = body['messages'] as import('../agents/types.js').ChatMessage[] | undefined;

      if (!messages || !Array.isArray(messages) || messages.length === 0) {
        res
          .status(400)
          .json({ error: { message: '`messages` array is required and must be non-empty' } });
        return;
      }

      // ── 2.1 Context Assembly (Story 6.3 / 8.4 / #308) ────────────────────
      const injectedBlocks: Array<{
        id: string;
        source: string;
        relevance: number;
        content: string;
      }> = [];

      try {
        if (messages) {
          messages = (await contextAssembler.assemble(
            agentId,
            messages as unknown as import('../llm/LlmRouter.js').ChatMessage[],
            (event) => {
              logger.info(`[context-assembly] ${event.stage}`, {
                ...event.detail,
                ...(event.durationMs !== undefined ? { durationMs: event.durationMs } : {}),
              });

              // Publish as thought event to Centrifugo/Audit stream (Story 305)
              const intercom = orchestrator.getIntercom();
              if (intercom) {
                const manifest =
                  orchestrator.getManifestByInstanceId(agentId) ||
                  orchestrator.getManifest(agentId);
                const displayName = manifest?.metadata.displayName ?? agentId;

                intercom
                  .publishThought(
                    agentId,
                    displayName,
                    'context-assembly',
                    `Context: ${event.stage}`,
                    undefined, // taskId unknown in proxy
                    undefined, // iteration unknown in proxy
                    event.detail
                  )
                  .catch((err) => logger.warn('Failed to publish assembly thought:', err));
              }

              // Collect injected memory blocks for citation metadata
              if (
                event.stage === 'context.memory_retrieved' &&
                Array.isArray(event.detail?.blocks)
              ) {
                injectedBlocks.push(...(event.detail.blocks as typeof injectedBlocks));
              }
            }
          )) as unknown as import('../agents/types.js').ChatMessage[];
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
            messages as unknown as import('../llm/LlmRouter.js').ChatMessage[],
            modelName,
            (event) => {
              logger.info(`[context-compaction] ${event.stage}`, {
                ...event.detail,
                ...(event.durationMs !== undefined ? { durationMs: event.durationMs } : {}),
              });
            }
          )) as unknown as import('../agents/types.js').ChatMessage[];
        }
      } catch (err) {
        logger.error('Context compaction failed (continuing with full context):', err);
      }

      const chatRequest = {
        model: modelName,
        messages: messages as unknown as import('../llm/LlmRouter.js').ChatMessage[],
        ...(temperature !== undefined ? { temperature: temperature as number } : {}),
        ...(Array.isArray(tools) ? { tools: tools as unknown[] } : {}),
        ...(thinkingLevel ? { thinkingLevel: thinkingLevel as string } : {}),
      };

      // ── 3. Streaming path ──────────────────────────────────────────────────
      if (stream === true) {
        // Pre-flight circuit breaker check for streaming
        const streamProvider = providerFromModel(modelName);
        const streamBreaker = circuitBreakerService.getProviderState(streamProvider);
        if (streamBreaker?.state === 'open') {
          res.status(503).json({
            error: 'provider_unavailable',
            provider: streamProvider,
            message: `Provider ${streamProvider} is currently unavailable (circuit open)`,
          });
          return;
        }

        const streamAbortController = new AbortController();

        try {
          logger.info(`Proxy stream | agent=${agentId} model=${modelName}`);
          const streamRes = await llmRouter.chatCompletionStream(
            { ...chatRequest, stream: true },
            agentId,
            streamAbortController.signal
          );

          res.setHeader('Content-Type', 'text/event-stream');
          res.setHeader('Cache-Control', 'no-cache');
          res.setHeader('Connection', 'keep-alive');
          res.setHeader('X-Accel-Buffering', 'no');

          // Prevent dead client connections from wasting upstream resources (5m timeout)
          res.socket?.setTimeout(300000);

          // ── Metering: intercept SSE chunks to count tokens ──────────────
          let streamedTokens = 0;
          const streamStart = Date.now();

          const meter = new PassThrough();

          meter.on('data', (chunk: Buffer) => {
            const text = chunk.toString('utf8');
            const lines = text.split('\n');
            for (const line of lines) {
              if (!line.startsWith('data: ')) continue;
              const jsonStr = line.slice(6).trim();
              if (jsonStr === '[DONE]') continue;
              try {
                const parsed = JSON.parse(jsonStr) as {
                  choices?: Array<{
                    delta?: {
                      content?: string;
                      tool_calls?: Array<{ function?: { arguments?: string } }>;
                    };
                  }>;
                  usage?: { completion_tokens?: number };
                };
                const delta = parsed.choices?.[0]?.delta;
                if (delta?.content) {
                  streamedTokens += Math.ceil(delta.content.length / 4);
                }
                if (delta?.tool_calls) {
                  for (const tc of delta.tool_calls) {
                    if (tc.function?.arguments) {
                      streamedTokens += Math.ceil(tc.function.arguments.length / 4);
                    }
                  }
                }
                // If the provider sends usage in the final chunk, prefer it
                if (parsed.usage) {
                  streamedTokens = parsed.usage.completion_tokens ?? streamedTokens;
                }
              } catch {
                // Not valid JSON — skip
              }
            }
          });

          meter.on('end', () => {
            const latencyMs = Date.now() - streamStart;
            meteringService
              .recordUsage({
                agentId,
                circleId,
                model: modelName,
                promptTokens: 0,
                completionTokens: streamedTokens,
                totalTokens: streamedTokens,
                latencyMs,
                status: 'success',
              })
              .catch((err) => logger.error('Failed to record streaming metering:', err));
          });

          const cleanup = (err?: Error) => {
            if (err) {
              logger.error(`LLM stream error | agent=${agentId}:`, err.message);
              meteringService
                .recordUsage({
                  agentId,
                  circleId,
                  model: modelName,
                  promptTokens: 0,
                  completionTokens: streamedTokens,
                  totalTokens: streamedTokens,
                  latencyMs: Date.now() - streamStart,
                  status: 'error',
                })
                .catch((merr) => logger.error('Failed to record streaming error metering:', merr));

              if (!res.headersSent) {
                const is429 =
                  err instanceof RateLimitedError ||
                  err.message?.includes('429') ||
                  err.message?.toLowerCase().includes('rate limit') ||
                  err.message?.toLowerCase().includes('rate_limit');
                if (is429) {
                  const retryAfterSec =
                    err instanceof RateLimitedError ? err.retryAfterSec : undefined;
                  const retryAfterMs = retryAfterSec !== undefined ? retryAfterSec * 1000 : 30000;
                  res.status(429).json({
                    error: 'rate_limited',
                    message:
                      'Upstream provider is rate-limited. Retry shortly or configure failover models.',
                    retryAfterMs,
                    ...(retryAfterSec !== undefined ? { retryAfter: retryAfterSec } : {}),
                  });
                } else {
                  res.status(502).json({
                    error: {
                      message: `Upstream LLM error: ${err.message}`,
                      type: 'upstream_error',
                    },
                  });
                }
              } else {
                res.end();
              }
            }
            streamAbortController.abort();
            if (!streamRes.destroyed) streamRes.destroy();
            if (!meter.destroyed) meter.destroy();
          };

          streamRes.on('error', cleanup);
          meter.on('error', cleanup);

          streamRes.on('close', () => {
            logger.debug(`Upstream stream closed | agent=${agentId}`);
          });

          // If client disconnects, destroy the upstream stream to save resources
          res.on('close', () => {
            if (!streamRes.destroyed) {
              logger.debug(`Client disconnected, destroying upstream stream | agent=${agentId}`);
              cleanup();
            }
          });

          streamRes.pipe(meter).pipe(res);
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
          const is429Stream =
            err instanceof RateLimitedError ||
            streamErr.message?.includes('429') ||
            streamErr.message?.toLowerCase().includes('rate limit') ||
            streamErr.message?.toLowerCase().includes('rate_limit');
          if (is429Stream) {
            logger.warn(`Rate-limited by upstream provider | agent=${agentId} model=${modelName}`);
            const retryAfterSec =
              err instanceof RateLimitedError ? (err as RateLimitedError).retryAfterSec : undefined;
            const retryAfterMs = retryAfterSec !== undefined ? retryAfterSec * 1000 : 30000;
            res.status(429).json({
              error: 'rate_limited',
              message:
                'Upstream provider is rate-limited. Retry shortly or configure failover models.',
              retryAfterMs,
              ...(retryAfterSec !== undefined ? { retryAfter: retryAfterSec } : {}),
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

        const is429 =
          err instanceof RateLimitedError ||
          cbErr.message?.includes('429') ||
          cbErr.message?.toLowerCase().includes('rate limit') ||
          cbErr.message?.toLowerCase().includes('rate_limit');
        if (is429) {
          logger.warn(`Rate-limited by upstream provider | agent=${agentId} model=${modelName}`);
          const retryAfterSec =
            err instanceof RateLimitedError ? (err as RateLimitedError).retryAfterSec : undefined;
          const retryAfterMs = retryAfterSec !== undefined ? retryAfterSec * 1000 : 30000;
          res.status(429).json({
            error: 'rate_limited',
            message:
              'Upstream provider is rate-limited. Retry shortly or configure failover models.',
            retryAfterMs,
            ...(retryAfterSec !== undefined ? { retryAfter: retryAfterSec } : {}),
          });
          return;
        }

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

        // ── 5a. Accumulate trace data (Story 30.1) ────────────────────────────
        // Non-blocking — record messages and token usage into the in-memory
        // accumulator keyed by agentId::sessionId.
        try {
          const requestMessages = chatRequest.messages as Array<{
            role: string;
            content: string;
          }>;
          for (const msg of requestMessages) {
            traceService.addMessage(agentId, sessionId, {
              role: msg.role as 'user' | 'assistant' | 'system' | 'tool',
              content: typeof msg.content === 'string' ? msg.content : JSON.stringify(msg.content),
              timestamp: new Date().toISOString(),
            });
          }
          const respChoice = (
            llmResponse.response.choices as Array<{
              message?: { role?: string; content?: string };
            }>
          )?.[0];
          if (respChoice?.message?.content) {
            traceService.addMessage(agentId, sessionId, {
              role: 'assistant',
              content: respChoice.message.content,
              timestamp: new Date().toISOString(),
            });
          }
          const usage = llmResponse.response.usage;
          traceService.recordTokens(
            agentId,
            sessionId,
            usage?.prompt_tokens ?? 0,
            usage?.completion_tokens ?? 0
          );
          traceService.setModel(agentId, sessionId, modelName);
          // Persist and reset accumulator for this turn
          traceService
            .persist(agentId, sessionId)
            .catch((err) => logger.error('Failed to persist interaction trace:', err));
        } catch (traceErr) {
          logger.warn('Trace accumulation error (non-fatal):', traceErr);
        }

        // ── 6. Return response ─────────────────────────────────────────────────
        logger.debug(
          `Proxy complete | agent=${agentId} model=${modelName} ` +
            `tokens=${usage?.total_tokens ?? 0} latency=${llmResponse.latencyMs}ms`
        );

        const response = { ...llmResponse.response } as Record<string, unknown>;
        const choice = (response['choices'] as any[])?.[0];
        const content = choice?.message?.content as string | undefined;

        if (content && injectedBlocks.length > 0) {
          const citations: Array<{ blockId: string; scope: string; relevance: number }> = [];
          const seenIds = new Set<string>();

          // Track by explicit citation: [from: id]
          const citationRegex = /\[from:\s*([^\]]+)\]/g;
          let match;
          while ((match = citationRegex.exec(content)) !== null) {
            const blockId = match[1]!.trim();
            const block = injectedBlocks.find((b) => b.id === blockId);
            if (block && !seenIds.has(block.id)) {
              citations.push({
                blockId: block.id,
                scope: block.source,
                relevance: block.relevance,
              });
              seenIds.add(block.id);
            }
          }

          // Track by content overlap if not explicitly cited (simple keyword/fragment match for 'brief' mode)
          // For now, we'll only use explicit citations or very basic overlap if no explicit citations found
          if (citations.length === 0) {
            for (const block of injectedBlocks) {
              if (seenIds.has(block.id)) continue;
              // Check for significant overlap — at least 30 chars and present in content
              if (block.content.length > 30) {
                const fragment = block.content.substring(0, 100).toLowerCase();
                // If a significant fragment of the memory block is in the response, count it
                const fragmentToCheck = fragment.substring(0, Math.min(fragment.length, 50));
                if (content.toLowerCase().includes(fragmentToCheck)) {
                  citations.push({
                    blockId: block.id,
                    scope: block.source,
                    relevance: block.relevance,
                  });
                  seenIds.add(block.id);
                }
              }
            }
          }

          if (citations.length > 0) {
            response['citations'] = citations;
          }
        }

        res.json(response);
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
