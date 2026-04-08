/**
 * ProviderHealthService — probes LLM provider endpoints for reachability
 * and model discovery.
 *
 * Inspired by OpenFang's provider_health.rs pattern:
 * - TTL-cached health checks (60s default)
 * - Per-provider discovery (list available models)
 * - Google AI Studio native model listing
 *
 * @see core/src/llm/ProviderRegistry.ts
 */

import type { ProviderConfig } from './ProviderRegistry.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('ProviderHealth');

export interface HealthStatus {
  reachable: boolean;
  latencyMs: number;
  error?: string;
  modelsAvailable?: string[];
}

interface CacheEntry {
  status: HealthStatus;
  checkedAt: number;
}

const TTL_MS = 60_000; // 60 seconds

export class ProviderHealthService {
  private cache = new Map<string, CacheEntry>();

  /**
   * Check health and discover models for a single provider config.
   * Returns cached result if within TTL.
   */
  async checkHealth(config: ProviderConfig): Promise<HealthStatus> {
    const cached = this.cache.get(config.modelName);
    if (cached && Date.now() - cached.checkedAt < TTL_MS) {
      return cached.status;
    }

    const status = await this.probe(config);
    this.cache.set(config.modelName, { status, checkedAt: Date.now() });
    return status;
  }

  /**
   * Discover models available at a provider's endpoint.
   * Works for OpenAI-compatible, Ollama, and Google AI Studio.
   */
  async discoverModels(
    config: ProviderConfig,
    registry?: { resolveApiKey: (c: ProviderConfig) => Promise<string | undefined> }
  ): Promise<string[]> {
    try {
      // Google AI Studio native models API
      if (config.provider === 'google') {
        return await this.discoverGoogleModels(config, registry);
      }

      // Ollama native API
      if (config.provider === 'ollama' && config.baseUrl) {
        const ollamaBase = config.baseUrl.replace(/\/v1\/?$/, '');
        return await this.discoverOllamaModels(ollamaBase);
      }

      // OpenAI-compatible /models endpoint
      if (config.baseUrl) {
        const resolvedVal = registry
          ? await registry.resolveApiKey(config)
          : this.resolveApiKeyLegacy(config);
        return await this.discoverOpenAIModels(config.baseUrl, resolvedVal);
      }

      return [];
    } catch (err) {
      logger.warn(`Model discovery failed for ${config.modelName}: ${(err as Error).message}`);
      return [];
    }
  }

  // ── Private probing methods ──────────────────────────────────────────────────

  private async probe(config: ProviderConfig): Promise<HealthStatus> {
    const start = Date.now();

    try {
      if (config.provider === 'google') {
        const models = await this.discoverGoogleModels(config);
        return {
          reachable: true,
          latencyMs: Date.now() - start,
          modelsAvailable: models,
        };
      }

      if (config.baseUrl) {
        const resolvedVal = this.resolveApiKeyLegacy(config);

        // Try /models endpoint first (OpenAI-compatible)
        try {
          const models = await this.discoverOpenAIModels(config.baseUrl, resolvedVal);
          return {
            reachable: true,
            latencyMs: Date.now() - start,
            modelsAvailable: models,
          };
        } catch {
          // Fall back to Ollama /api/tags
          if (config.provider === 'ollama') {
            const ollamaBase = config.baseUrl.replace(/\/v1\/?$/, '');
            try {
              const models = await this.discoverOllamaModels(ollamaBase);
              return {
                reachable: true,
                latencyMs: Date.now() - start,
                modelsAvailable: models,
              };
            } catch {
              // Both failed
            }
          }
        }
      }

      // Cloud providers without baseUrl — check standard endpoint
      if (config.provider === 'openai') {
        const res = await fetchWithTimeout('https://api.openai.com/v1/models', {
          headers: { Authorization: `Bearer ${process.env.OPENAI_API_KEY ?? ''}` },
        });
        const result: HealthStatus = {
          reachable: res.ok,
          latencyMs: Date.now() - start,
        };
        if (!res.ok) result.error = `HTTP ${res.status}`;
        return result;
      }

      if (config.provider === 'anthropic') {
        // Anthropic doesn't have a /models endpoint; just check auth
        const res = await fetchWithTimeout('https://api.anthropic.com/v1/messages', {
          method: 'POST',
          headers: {
            'x-api-key': process.env.ANTHROPIC_API_KEY ?? '',
            'anthropic-version': '2023-06-01',
            'content-type': 'application/json',
          },
          body: JSON.stringify({
            model: 'claude-haiku-4-5',
            max_tokens: 1,
            messages: [{ role: 'user', content: 'hi' }],
          }),
        });
        // 200 or 400 (bad request but auth worked) = reachable
        const authFailed = res.status === 401 || res.status === 403;
        const result: HealthStatus = {
          reachable: !authFailed,
          latencyMs: Date.now() - start,
        };
        if (authFailed) result.error = 'Authentication failed';
        return result;
      }

      return {
        reachable: false,
        latencyMs: Date.now() - start,
        error: 'No probe method available for this provider',
      };
    } catch (err) {
      return {
        reachable: false,
        latencyMs: Date.now() - start,
        error: (err as Error).message,
      };
    }
  }

  private async discoverOpenAIModels(baseUrl: string, inputKey?: string): Promise<string[]> {
    const headers: Record<string, string> = {};
    if (inputKey) headers['Authorization'] = `Bearer ${inputKey}`;

    const res = await fetchWithTimeout(`${baseUrl}/models`, { headers });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);

    const data = (await res.json()) as { data?: { id: string }[] };
    return (data.data ?? []).map((m) => m.id);
  }

  private async discoverOllamaModels(baseUrl: string): Promise<string[]> {
    const res = await fetchWithTimeout(`${baseUrl}/api/tags`);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);

    const data = (await res.json()) as { models?: { name: string }[] };
    return (data.models ?? []).map((m) => m.name);
  }

  private async discoverGoogleModels(
    config: ProviderConfig,
    registry?: { resolveApiKey: (c: ProviderConfig) => Promise<string | undefined> }
  ): Promise<string[]> {
    const resolvedVal = registry
      ? await registry.resolveApiKey(config)
      : this.resolveApiKeyLegacy(config);
    if (!resolvedVal) throw new Error('GOOGLE_API_KEY not configured');

    const res = await fetchWithTimeout(
      `https://generativelanguage.googleapis.com/v1beta/models?key=${resolvedVal}`
    );
    if (!res.ok) throw new Error(`HTTP ${res.status}`);

    const data = (await res.json()) as {
      models?: { name: string; supportedGenerationMethods?: string[] }[];
    };

    return (data.models ?? [])
      .filter((m) => m.supportedGenerationMethods?.includes('generateContent'))
      .map((m) => m.name.replace('models/', ''));
  }

  /** @deprecated use registry.resolveApiKey for secret support */
  private resolveApiKeyLegacy(config: ProviderConfig): string | undefined {
    if (config.apiKey) return config.apiKey;
    if (config.apiKeyEnvVar) return process.env[config.apiKeyEnvVar];

    const standardEnvVars: Record<string, string[]> = {
      openai: ['OPENAI_API_KEY'],
      anthropic: ['ANTHROPIC_API_KEY'],
      google: ['GOOGLE_API_KEY', 'GEMINI_API_KEY'],
      groq: ['GROQ_API_KEY'],
      mistral: ['MISTRAL_API_KEY'],
      openrouter: ['OPENROUTER_API_KEY'],
      kilocode: ['KILOCODE_API_KEY'],
    };
    if (config.provider) {
      const envVars = standardEnvVars[config.provider];
      if (envVars) {
        for (const v of envVars) {
          if (process.env[v]) return process.env[v];
        }
      }
    }
    return undefined;
  }
}

async function fetchWithTimeout(url: string, init?: RequestInit): Promise<Response> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 10_000);
  try {
    return await fetch(url, { ...init, signal: controller.signal });
  } finally {
    clearTimeout(timeout);
  }
}
