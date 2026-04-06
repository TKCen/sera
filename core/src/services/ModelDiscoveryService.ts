/**
 * ModelDiscoveryService — queries cloud providers' /models endpoints and caches results.
 *
 * Supported providers: openrouter, google, kilocode
 * Cache TTL: 1 hour per provider
 */

import { Logger } from '../lib/logger.js';

const logger = new Logger('ModelDiscoveryService');

const CACHE_TTL_MS = 60 * 60 * 1000; // 1 hour

export interface DiscoveredModel {
  id: string;
  name: string;
  provider: string;
  contextWindow?: number;
  pricing?: { input: number; output: number }; // per million tokens
  isFree?: boolean;
  capabilities?: string[];
}

interface ProviderCache {
  models: DiscoveredModel[];
  lastFetched: number;
}

// ── OpenRouter response shapes ─────────────────────────────────────────────

interface OpenRouterModel {
  id: string;
  name: string;
  context_length?: number;
  pricing?: {
    prompt?: string;
    completion?: string;
  };
  architecture?: {
    modality?: string;
    tokenizer?: string;
    instruct_type?: string | null;
  };
}

interface OpenRouterResponse {
  data: OpenRouterModel[];
}

// ── Google AI Studio response shapes ──────────────────────────────────────

interface GoogleModel {
  name: string;
  displayName?: string;
  inputTokenLimit?: number;
  supportedGenerationMethods?: string[];
}

interface GoogleModelsResponse {
  models: GoogleModel[];
}

// ── Kilo Code response shapes ──────────────────────────────────────────────

interface KiloModel {
  id: string;
  name?: string;
}

interface KiloModelsResponse {
  data?: KiloModel[];
  models?: KiloModel[];
}

// ── Service ───────────────────────────────────────────────────────────────

export class ModelDiscoveryService {
  private static instance: ModelDiscoveryService;
  private readonly cache = new Map<string, ProviderCache>();

  private constructor() {}

  static getInstance(): ModelDiscoveryService {
    if (!ModelDiscoveryService.instance) {
      ModelDiscoveryService.instance = new ModelDiscoveryService();
    }
    return ModelDiscoveryService.instance;
  }

  /**
   * Discover models for a specific provider.
   * Returns cached results if within TTL unless force=true.
   */
  async discoverModels(provider: string, force = false): Promise<DiscoveredModel[]> {
    const cached = this.cache.get(provider);
    if (!force && cached && Date.now() - cached.lastFetched < CACHE_TTL_MS) {
      logger.debug(`Returning cached models for provider ${provider}`);
      return cached.models;
    }

    let models: DiscoveredModel[];
    switch (provider) {
      case 'openrouter':
        models = await this.fetchOpenRouterModels();
        break;
      case 'google':
        models = await this.fetchGoogleModels();
        break;
      case 'kilocode':
        models = await this.fetchKiloCodeModels();
        break;
      default:
        logger.warn(`Unknown provider for model discovery: ${provider}`);
        return [];
    }

    this.cache.set(provider, { models, lastFetched: Date.now() });
    return models;
  }

  /**
   * Discover models for all supported providers.
   * Returns a map of provider -> models.
   */
  async discoverAll(force = false): Promise<Record<string, DiscoveredModel[]>> {
    const providers = ['openrouter', 'google', 'kilocode'];
    const results = await Promise.all(
      providers.map(async (p) => {
        const models = await this.discoverModels(p, force);
        return [p, models] as const;
      })
    );
    return Object.fromEntries(results);
  }

  // ── Provider fetchers ─────────────────────────────────────────────────────

  private async fetchOpenRouterModels(): Promise<DiscoveredModel[]> {
    try {
      const response = await fetch('https://openrouter.ai/api/v1/models', {
        headers: { 'Content-Type': 'application/json' },
        signal: AbortSignal.timeout(15000),
      });

      if (!response.ok) {
        logger.warn(`OpenRouter models endpoint returned ${response.status}`);
        return [];
      }

      const data = (await response.json()) as OpenRouterResponse;

      return data.data.map((m): DiscoveredModel => {
        const promptPrice = parseFloat(m.pricing?.prompt ?? '0');
        const completionPrice = parseFloat(m.pricing?.completion ?? '0');
        const isFree = promptPrice === 0 && completionPrice === 0;

        const capabilities: string[] = [];
        const modality = m.architecture?.modality ?? '';
        if (modality.includes('image')) capabilities.push('vision');
        if (modality.includes('text->text') || modality === '') {
          // base text capability — no special tag
        }

        return {
          id: m.id,
          name: m.name,
          provider: 'openrouter',
          ...(m.context_length !== undefined ? { contextWindow: m.context_length } : {}),
          pricing: {
            input: promptPrice * 1_000_000,
            output: completionPrice * 1_000_000,
          },
          isFree,
          ...(capabilities.length > 0 ? { capabilities } : {}),
        };
      });
    } catch (err: unknown) {
      logger.warn(`Failed to fetch OpenRouter models: ${(err as Error).message}`);
      return [];
    }
  }

  private async fetchGoogleModels(): Promise<DiscoveredModel[]> {
    const apiKey = process.env.GOOGLE_API_KEY ?? process.env.GEMINI_API_KEY;
    if (!apiKey) {
      logger.warn('No GOOGLE_API_KEY or GEMINI_API_KEY set — skipping Google model discovery');
      return [];
    }

    try {
      const url = `https://generativelanguage.googleapis.com/v1beta/models?key=${apiKey}`;
      const response = await fetch(url, {
        signal: AbortSignal.timeout(15000),
      });

      if (!response.ok) {
        logger.warn(`Google models endpoint returned ${response.status}`);
        return [];
      }

      const data = (await response.json()) as GoogleModelsResponse;

      return (data.models ?? []).map((m): DiscoveredModel => {
        // Strip "models/" prefix from name to get the usable model ID
        const id = m.name.startsWith('models/') ? m.name.slice('models/'.length) : m.name;

        const capabilities: string[] = [];
        const methods = m.supportedGenerationMethods ?? [];
        if (methods.includes('generateContent')) capabilities.push('tools');

        return {
          id,
          name: m.displayName ?? id,
          provider: 'google',
          ...(m.inputTokenLimit !== undefined ? { contextWindow: m.inputTokenLimit } : {}),
          ...(capabilities.length > 0 ? { capabilities } : {}),
        };
      });
    } catch (err: unknown) {
      logger.warn(`Failed to fetch Google models: ${(err as Error).message}`);
      return [];
    }
  }

  private async fetchKiloCodeModels(): Promise<DiscoveredModel[]> {
    const apiKey = process.env.KILOCODE_API_KEY;
    if (!apiKey) {
      logger.warn('No KILOCODE_API_KEY set — returning hardcoded Kilo Code model list');
      return this.kiloCodeFallback();
    }

    try {
      const response = await fetch('https://api.kilo.ai/api/gateway/models', {
        headers: {
          Authorization: `Bearer ${apiKey}`,
          'Content-Type': 'application/json',
        },
        signal: AbortSignal.timeout(15000),
      });

      if (!response.ok) {
        logger.warn(
          `Kilo Code models endpoint returned ${response.status} — falling back to hardcoded list`
        );
        return this.kiloCodeFallback();
      }

      const data = (await response.json()) as KiloModelsResponse;
      const rawModels = data.data ?? data.models ?? [];

      if (rawModels.length === 0) {
        return this.kiloCodeFallback();
      }

      return rawModels.map(
        (m): DiscoveredModel => ({
          id: m.id,
          name: m.name ?? m.id,
          provider: 'kilocode',
        })
      );
    } catch (err: unknown) {
      logger.warn(
        `Failed to fetch Kilo Code models: ${(err as Error).message} — returning hardcoded list`
      );
      return this.kiloCodeFallback();
    }
  }

  private kiloCodeFallback(): DiscoveredModel[] {
    // Discovery not available — return known supported models
    return [
      { id: 'gpt-4o', name: 'GPT-4o', provider: 'kilocode' },
      {
        id: 'claude-sonnet-4-20250514',
        name: 'Claude Sonnet 4 (2025-05-14)',
        provider: 'kilocode',
      },
    ];
  }
}
