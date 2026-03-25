import { Logger } from '../lib/logger.js';
import { EmbeddingRouter } from './embedding-router.js';
import { loadEmbeddingConfig } from './embedding-config.js';
import type { EmbeddingConfig } from './embedding-config.js';

const logger = new Logger('EmbeddingService');

interface QueueEntry {
  text: string;
  resolve: (v: number[]) => void;
  reject: (e: unknown) => void;
}

/**
 * EmbeddingService — generates vector embeddings via configurable providers.
 *
 * Delegates to EmbeddingRouter for actual HTTP dispatch. Supports Ollama,
 * OpenAI, LM Studio, and any OpenAI-compatible endpoint.
 *
 * Requests are queued to avoid flooding the provider and are retried with
 * exponential backoff if the provider is temporarily unavailable.
 */
export class EmbeddingService {
  private static instance: EmbeddingService;
  private router: EmbeddingRouter;
  private queue: QueueEntry[] = [];
  private processing = false;
  private available = true;

  private constructor(router?: EmbeddingRouter) {
    this.router = router ?? new EmbeddingRouter(loadEmbeddingConfig());
  }

  static getInstance(router?: EmbeddingRouter): EmbeddingService {
    if (!EmbeddingService.instance) {
      EmbeddingService.instance = new EmbeddingService(router);
    }
    return EmbeddingService.instance;
  }

  /** Get the current embedding dimension. */
  static getDimension(): number {
    return EmbeddingService.getInstance().router.getDimension();
  }

  /** Get the underlying router (for routes to access config). */
  getRouter(): EmbeddingRouter {
    return this.router;
  }

  /** True only when provider is configured AND connection succeeded. */
  isAvailable(): boolean {
    return this.available;
  }

  /** True when a provider baseUrl is configured. */
  isConfigured(): boolean {
    return this.router.isConfigured();
  }

  /** Hot-swap the embedding config at runtime. */
  reconfigure(config: EmbeddingConfig): void {
    this.router.updateConfig(config);
    this.available = true; // reset — warmup will verify
    logger.info(`Reconfigured: ${config.provider}/${config.model} (${config.dimension}d)`);
  }

  async embed(text: string): Promise<number[]> {
    if (!this.available) {
      throw new Error('EmbeddingService: provider unavailable — RAG disabled');
    }
    return new Promise<number[]>((resolve, reject) => {
      this.queue.push({ text, resolve, reject });
      if (!this.processing) {
        void this.drain();
      }
    });
  }

  /** Backward compat alias for MemoryManager. */
  async generateEmbedding(text: string): Promise<number[]> {
    return this.embed(text);
  }

  private async drain(): Promise<void> {
    this.processing = true;
    while (this.queue.length > 0) {
      const entry = this.queue.shift()!;
      try {
        const vector = await this.callWithRetry(entry.text);
        entry.resolve(vector);
      } catch (err) {
        entry.reject(err);
      }
    }
    this.processing = false;
  }

  private async callWithRetry(text: string, attempt = 0): Promise<number[]> {
    const maxAttempts = 5;
    const start = Date.now();
    try {
      const vector = await this.router.embed(text);
      const elapsed = Date.now() - start;
      if (elapsed > 500) {
        logger.warn(`Embedding generation took ${elapsed}ms (>500ms threshold)`);
      } else {
        logger.debug(`Embedding generated in ${elapsed}ms`);
      }
      this.available = true;
      return vector;
    } catch (err) {
      if (attempt >= maxAttempts - 1) {
        logger.error(`Provider unreachable after ${maxAttempts} attempts — disabling RAG`);
        this.available = false;
        throw err;
      }
      const delay = Math.min(1000 * 2 ** attempt, 30_000);
      logger.warn(`Attempt ${attempt + 1} failed, retrying in ${delay}ms`, err);
      await new Promise((r) => setTimeout(r, delay));
      return this.callWithRetry(text, attempt + 1);
    }
  }

  /**
   * Warm up: verify the embedding provider is reachable at startup.
   * Skipped when no baseUrl is configured. Logs but does not throw.
   */
  async warmup(): Promise<void> {
    if (!this.router.isConfigured()) {
      const config = this.router.getConfig();
      logger.info(
        `EmbeddingService: no baseUrl configured — embeddings disabled. Configure an embedding provider to enable RAG.`
      );
      this.available = false;
      return;
    }
    try {
      const result = await this.router.testConnection();
      if (result.ok) {
        const config = this.router.getConfig();
        logger.info(
          `EmbeddingService ready (provider: ${config.provider}, model: ${config.model}, dimension: ${result.dimension}, latency: ${result.latencyMs}ms)`
        );
        this.available = true;
      } else {
        throw new Error(result.error ?? 'Test connection failed');
      }
    } catch (err) {
      const config = this.router.getConfig();
      logger.warn(
        `EmbeddingService: provider not reachable at ${config.baseUrl} — RAG disabled until provider comes online`
      );
      this.available = false;
    }
  }
}

/** Dynamic dimension getter — use instead of the old EMBEDDING_VECTOR_SIZE constant. */
export function getEmbeddingDimension(): number {
  return EmbeddingService.getDimension();
}

// Backward compat export — deprecated, use getEmbeddingDimension() instead
export const EMBEDDING_VECTOR_SIZE = 768;
