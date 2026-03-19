/**
 * CircuitBreakerService — per-provider opossum circuit breakers for LLM calls.
 *
 * Wraps each LiteLLM call with a circuit breaker keyed by provider (derived
 * from the model name prefix). If a provider fails repeatedly, the circuit
 * opens and calls fail immediately with 503 until the cool-down passes.
 *
 * Configuration defaults:
 *   - errorThresholdPercentage: 50 (open after 50% failures in rolling window)
 *   - rollingCountTimeout:      60_000ms (1 minute window)
 *   - resetTimeout:             30_000ms (30s cool-down before half-open)
 *   - volumeThreshold:          5 (minimum calls before circuit can open)
 *
 * @see docs/epics/04-llm-proxy-and-governance.md § Story 4.6
 */

import CircuitBreaker from 'opossum';
import { Logger } from '../lib/logger.js';
import type { LiteLLMClient, ChatCompletionRequest, ChatCompletionResponse } from './LiteLLMClient.js';

const logger = new Logger('CircuitBreakerService');

// ── Types ─────────────────────────────────────────────────────────────────────

export interface CircuitBreakerState {
  provider: string;
  state: 'closed' | 'open' | 'half-open';
  stats: {
    fires: number;
    failures: number;
    successes: number;
    rejects: number;
    timeouts: number;
  };
}

type LlmCallFn = (
  request: ChatCompletionRequest,
  agentId: string,
  latencyStart: number,
) => Promise<{ response: ChatCompletionResponse; latencyMs: number }>;

// ── Provider extraction ───────────────────────────────────────────────────────

/**
 * Derive the provider key from a model name.
 * Examples: 'lmstudio-default' → 'lmstudio', 'gpt-4o-mini' → 'openai', 'claude-haiku' → 'anthropic'
 *
 * DECISION: We group by well-known prefixes for cloud providers; everything
 * else uses the first dash-segment. This keeps circuit breaking per-provider
 * without requiring explicit configuration.
 */
export function providerFromModel(model: string): string {
  const lower = model.toLowerCase();
  if (lower.startsWith('gpt-') || lower.startsWith('o1') || lower.startsWith('o3')) return 'openai';
  if (lower.startsWith('claude-')) return 'anthropic';
  if (lower.startsWith('gemini-')) return 'google';
  if (lower.startsWith('ollama-') || lower.includes('llama')) return 'ollama';
  if (lower.startsWith('lmstudio')) return 'lmstudio';
  // Fallback: first segment before a dash or the whole model name
  const dash = lower.indexOf('-');
  return dash > 0 ? lower.slice(0, dash) : lower;
}

// ── Service ───────────────────────────────────────────────────────────────────

export class CircuitBreakerService {
  private readonly breakers = new Map<string, CircuitBreaker<Parameters<LlmCallFn>, Awaited<ReturnType<LlmCallFn>>>>();
  private readonly client: LiteLLMClient;

  private readonly options: CircuitBreaker.Options = {
    errorThresholdPercentage: 50,
    rollingCountTimeout: 60_000,
    resetTimeout: 30_000,
    volumeThreshold: 5,
    timeout: 120_000,
  };

  constructor(client: LiteLLMClient) {
    this.client = client;
  }

  /** Get or create a circuit breaker for the given provider. */
  private getBreakerForProvider(provider: string): CircuitBreaker<Parameters<LlmCallFn>, Awaited<ReturnType<LlmCallFn>>> {
    let breaker = this.breakers.get(provider);
    if (!breaker) {
      const fn: LlmCallFn = (req, agentId, latencyStart) =>
        this.client.chatCompletion(req, agentId, latencyStart);

      breaker = new CircuitBreaker(fn, {
        ...this.options,
        name: `llm-${provider}`,
      });

      breaker.on('open', () => {
        logger.warn(`Circuit OPENED for provider=${provider}`);
      });
      breaker.on('halfOpen', () => {
        logger.info(`Circuit HALF-OPEN for provider=${provider} — testing...`);
      });
      breaker.on('close', () => {
        logger.info(`Circuit CLOSED for provider=${provider} — provider recovered`);
      });

      this.breakers.set(provider, breaker);
    }
    return breaker;
  }

  /**
   * Execute a chat completion through the provider's circuit breaker.
   * Throws with { code: 'CIRCUIT_OPEN' } if the circuit is open.
   */
  async call(
    request: ChatCompletionRequest,
    agentId: string,
    latencyStart: number = Date.now(),
  ): Promise<{ response: ChatCompletionResponse; latencyMs: number }> {
    const provider = providerFromModel(request.model);
    const breaker = this.getBreakerForProvider(provider);

    try {
      return await breaker.fire(request, agentId, latencyStart);
    } catch (err: any) {
      if (err.code === 'EOPENBREAKER' || err.message?.includes('Breaker is open')) {
        const circuitErr = new Error(`Provider ${provider} is currently unavailable (circuit open)`);
        (circuitErr as any).code = 'CIRCUIT_OPEN';
        (circuitErr as any).provider = provider;
        throw circuitErr;
      }
      throw err;
    }
  }

  /**
   * Return current state of all known circuit breakers.
   * Used by GET /api/system/circuit-breakers.
   */
  getState(): CircuitBreakerState[] {
    const states: CircuitBreakerState[] = [];
    for (const [provider, breaker] of this.breakers.entries()) {
      const stats = breaker.stats;
      states.push({
        provider,
        state: breaker.opened ? 'open' : breaker.halfOpen ? 'half-open' : 'closed',
        stats: {
          fires: stats.fires,
          failures: stats.failures,
          successes: stats.successes,
          rejects: stats.rejects,
          timeouts: stats.timeouts,
        },
      });
    }
    return states;
  }

  /**
   * Return state for a specific provider's circuit breaker.
   * Returns null if no breaker exists for that provider yet.
   */
  getProviderState(provider: string): CircuitBreakerState | null {
    const breaker = this.breakers.get(provider);
    if (!breaker) return null;
    const stats = breaker.stats;
    return {
      provider,
      state: breaker.opened ? 'open' : breaker.halfOpen ? 'half-open' : 'closed',
      stats: {
        fires: stats.fires,
        failures: stats.failures,
        successes: stats.successes,
        rejects: stats.rejects,
        timeouts: stats.timeouts,
      },
    };
  }
}
