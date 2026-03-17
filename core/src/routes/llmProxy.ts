/**
 * LLM Proxy Routes — OpenAI-compatible gateway for agent containers.
 *
 * Agents call these endpoints instead of upstream LLM providers directly.
 * Core injects the real API key, records token usage, and enforces budgets.
 *
 * Endpoints:
 *   POST /v1/llm/chat/completions — proxied chat completion
 *   GET  /v1/llm/models           — list available models
 *
 * @see docs/v2-distributed-architecture/02-security-and-gateway.md § LLM Proxy Gateway
 */

import { Router } from 'express';
import type { Request, Response } from 'express';
import { ProviderFactory } from '../lib/llm/ProviderFactory.js';
import { config } from '../lib/config.js';
import { PROVIDER_CATALOG } from '../lib/providers.js';
import type { IdentityService } from '../auth/IdentityService.js';
import { createAuthMiddleware } from '../auth/authMiddleware.js';
import type { MeteringService } from '../metering/MeteringService.js';
import type { ChatMessage } from '../agents/types.js';
import type { ToolDefinition } from '../lib/llm/types.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('LLMProxy');

export function createLlmProxyRouter(
  identityService: IdentityService,
  meteringService: MeteringService,
): Router {
  const router = Router();
  const authMiddleware = createAuthMiddleware(identityService);

  // ── POST /chat/completions ─────────────────────────────────────────────────

  router.post('/chat/completions', authMiddleware, async (req: Request, res: Response) => {
    const identity = req.agentIdentity!;

    // ── Budget gate ──────────────────────────────────────────────────────────
    try {
      const budget = await meteringService.checkBudget(identity.agentId);
      if (!budget.allowed) {
        logger.warn(
          `Budget exceeded | agent=${identity.agentId} ` +
          `hourly=${budget.hourlyUsed}/${budget.hourlyQuota} ` +
          `daily=${budget.dailyUsed}/${budget.dailyQuota}`,
        );
        res.status(429).json({
          error: {
            message: 'Token budget exceeded',
            type: 'rate_limit_exceeded',
            hourlyUsed: budget.hourlyUsed,
            hourlyQuota: budget.hourlyQuota,
            dailyUsed: budget.dailyUsed,
            dailyQuota: budget.dailyQuota,
          },
        });
        return;
      }
    } catch (err: any) {
      logger.error('Budget check failed (allowing request):', err);
      // Fail-open: if metering DB is down, still allow the request
    }

    // ── Parse the OpenAI-shaped request ──────────────────────────────────────
    const {
      model,
      messages,
      temperature,
      tools: rawTools,
    } = req.body;

    if (!messages || !Array.isArray(messages)) {
      res.status(400).json({ error: { message: '`messages` array is required' } });
      return;
    }

    // Map request messages to internal ChatMessage format
    const chatMessages: ChatMessage[] = messages.map((m: any) => ({
      role: m.role,
      content: m.content ?? '',
      tool_calls: m.tool_calls,
      tool_call_id: m.tool_call_id,
    }));

    // Map tools if provided
    const tools: ToolDefinition[] | undefined = rawTools?.map((t: any) => ({
      type: 'function' as const,
      function: {
        name: t.function.name,
        description: t.function.description ?? '',
        parameters: t.function.parameters ?? {},
      },
    }));

    // ── Resolve the provider ─────────────────────────────────────────────────
    const modelName = model || config.llm.model;
    const activeProviderConfig = config.providers;
    const providerId = activeProviderConfig.activeProvider;
    const providerConfig = config.getProviderConfig(providerId);

    let provider;
    try {
      provider = ProviderFactory.createFromModelConfig({
        provider: providerId,
        name: modelName,
        temperature: temperature ?? undefined,
      });
    } catch (err: any) {
      logger.error('Provider creation failed:', err);
      res.status(500).json({ error: { message: `Failed to create LLM provider: ${err.message}` } });
      return;
    }

    // ── Call the LLM ─────────────────────────────────────────────────────────
    try {
      logger.info(
        `Proxy request | agent=${identity.agentId} model=${modelName} ` +
        `messages=${chatMessages.length} tools=${tools?.length ?? 0}`,
      );

      const response = await provider.chat(chatMessages, tools);

      // ── Record metering ──────────────────────────────────────────────────
      if (response.usage) {
        meteringService.recordUsage({
          agentId: identity.agentId,
          circleId: identity.circleId || null,
          model: modelName,
          promptTokens: response.usage.promptTokens,
          completionTokens: response.usage.completionTokens,
          totalTokens: response.usage.totalTokens,
        }).catch(err => {
          logger.error('Failed to record metering:', err);
        });
      }

      // ── Return OpenAI-compatible response ────────────────────────────────
      const openAIResponse: Record<string, unknown> = {
        id: `sera-${Date.now()}`,
        object: 'chat.completion',
        created: Math.floor(Date.now() / 1000),
        model: modelName,
        choices: [
          {
            index: 0,
            message: {
              role: 'assistant',
              content: response.content || null,
              ...(response.toolCalls && response.toolCalls.length > 0
                ? {
                    tool_calls: response.toolCalls.map(tc => ({
                      id: tc.id,
                      type: 'function',
                      function: {
                        name: tc.function.name,
                        arguments: tc.function.arguments,
                      },
                    })),
                  }
                : {}),
            },
            finish_reason: response.toolCalls && response.toolCalls.length > 0
              ? 'tool_calls'
              : 'stop',
          },
        ],
        usage: response.usage
          ? {
              prompt_tokens: response.usage.promptTokens,
              completion_tokens: response.usage.completionTokens,
              total_tokens: response.usage.totalTokens,
            }
          : undefined,
      };

      res.json(openAIResponse);
    } catch (err: any) {
      logger.error(`LLM proxy error | agent=${identity.agentId}:`, err);
      res.status(502).json({
        error: {
          message: `Upstream LLM error: ${err.message}`,
          type: 'upstream_error',
        },
      });
    }
  });

  // ── GET /models ────────────────────────────────────────────────────────────

  router.get('/models', authMiddleware, (_req: Request, res: Response) => {
    const models = PROVIDER_CATALOG.flatMap(provider =>
      provider.models.map(m => ({
        id: m.id,
        object: 'model',
        owned_by: provider.id,
        provider_name: provider.name,
      })),
    );
    res.json({ object: 'list', data: models });
  });

  return router;
}
