import { Logger } from '../lib/logger.js';
import type { EmbeddingConfig } from './embedding-config.js';
import { resolveApiKey } from './embedding-config.js';

const logger = new Logger('EmbeddingRouter');

interface OllamaEmbedResponse {
  embedding: number[];
}

interface OpenAIEmbedResponse {
  data: Array<{ embedding: number[]; index: number }>;
  model: string;
  usage: { prompt_tokens: number; total_tokens: number };
}

export interface TestResult {
  ok: boolean;
  latencyMs: number;
  dimension?: number;
  error?: string;
}

/**
 * EmbeddingRouter — lightweight HTTP dispatcher for embedding providers.
 * Supports Ollama, OpenAI, LM Studio, and any OpenAI-compatible endpoint.
 * No pi-mono dependency — embedding APIs are simple REST calls.
 */
export class EmbeddingRouter {
  private config: EmbeddingConfig;

  constructor(config: EmbeddingConfig) {
    this.config = config;
  }

  getConfig(): EmbeddingConfig {
    return this.config;
  }

  getDimension(): number {
    return this.config.dimension;
  }

  updateConfig(config: EmbeddingConfig): void {
    this.config = config;
    logger.info(`Config updated: ${config.provider}/${config.model} (${config.dimension}d)`);
  }

  isConfigured(): boolean {
    return !!this.config.baseUrl;
  }

  async embed(text: string): Promise<number[]> {
    if (!this.config.baseUrl) {
      throw new Error('EmbeddingRouter: no baseUrl configured');
    }
    if (this.config.provider === 'ollama') {
      return this.callOllama(text);
    }
    // openai, lm-studio, openai-compatible all use the same API
    return this.callOpenAI(text);
  }

  async testConnection(): Promise<TestResult> {
    const start = Date.now();
    try {
      const vector = await this.embed('test embedding connection');
      return {
        ok: true,
        latencyMs: Date.now() - start,
        dimension: vector.length,
      };
    } catch (err) {
      return {
        ok: false,
        latencyMs: Date.now() - start,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  }

  private async callOllama(text: string): Promise<number[]> {
    const url = `${this.config.baseUrl.replace(/\/$/, '')}/api/embeddings`;
    const res = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ model: this.config.model, prompt: text }),
      signal: AbortSignal.timeout(30_000),
    });
    if (!res.ok) {
      throw new Error(`Ollama HTTP ${res.status}: ${await res.text()}`);
    }
    const data = (await res.json()) as OllamaEmbedResponse;
    return data.embedding;
  }

  private async callOpenAI(text: string): Promise<number[]> {
    const baseUrl = this.config.baseUrl.replace(/\/$/, '');
    const url = `${baseUrl}/v1/embeddings`;
    const apiKey = resolveApiKey(this.config);

    const headers: Record<string, string> = { 'Content-Type': 'application/json' };
    if (apiKey) {
      headers['Authorization'] = `Bearer ${apiKey}`;
    }

    const res = await fetch(url, {
      method: 'POST',
      headers,
      body: JSON.stringify({ model: this.config.model, input: text }),
      signal: AbortSignal.timeout(30_000),
    });
    if (!res.ok) {
      throw new Error(`Embedding API HTTP ${res.status}: ${await res.text()}`);
    }
    const data = (await res.json()) as OpenAIEmbedResponse;
    if (!data.data?.[0]?.embedding) {
      throw new Error('Unexpected response format — missing data[0].embedding');
    }
    return data.data[0].embedding;
  }
}
