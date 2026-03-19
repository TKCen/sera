import { Logger } from '../lib/logger.js';

const logger = new Logger('EmbeddingService');

const OLLAMA_URL = process.env.OLLAMA_URL; // undefined → not configured
const EMBEDDING_MODEL = process.env.EMBEDDING_MODEL ?? 'nomic-embed-text';
const VECTOR_SIZE = 768; // nomic-embed-text produces 768-dim vectors

// DECISION: nomic-embed-text outputs 768 dimensions. The epic spec mentions
// 1536-dim (ada-002 compat) but the assignment rules mandate Ollama + nomic-embed-text.
// We use 768. To switch to a 1536-dim model, set EMBEDDING_MODEL to one that
// produces 1536 dimensions and update VECTOR_SIZE accordingly.
export const EMBEDDING_VECTOR_SIZE = VECTOR_SIZE;

interface OllamaEmbedResponse {
  embedding: number[];
}

interface QueueEntry {
  text: string;
  resolve: (v: number[]) => void;
  reject: (e: unknown) => void;
}

/**
 * EmbeddingService — generates vector embeddings via Ollama.
 *
 * Requests are queued to avoid flooding Ollama and are retried with
 * exponential backoff if Ollama is temporarily unavailable.
 * If Ollama is permanently unavailable, embed() rejects so callers
 * can disable RAG gracefully.
 */
export class EmbeddingService {
  private static instance: EmbeddingService;
  private queue: QueueEntry[] = [];
  private processing = false;
  private available = true; // flips false after all retries exhausted

  private constructor() {}

  static getInstance(): EmbeddingService {
    if (!EmbeddingService.instance) {
      EmbeddingService.instance = new EmbeddingService();
    }
    return EmbeddingService.instance;
  }

  /** True only when OLLAMA_URL is configured AND connection succeeded. */
  isAvailable(): boolean {
    return this.available;
  }

  /** True when OLLAMA_URL env var is explicitly set. */
  isConfigured(): boolean {
    return !!OLLAMA_URL;
  }

  async embed(text: string): Promise<number[]> {
    if (!this.available) {
      throw new Error('EmbeddingService: Ollama unavailable — RAG disabled');
    }
    return new Promise<number[]>((resolve, reject) => {
      this.queue.push({ text, resolve, reject });
      if (!this.processing) {
        void this.drain();
      }
    });
  }

  // Kept for backward compat with existing MemoryManager calls
  async generateEmbedding(text: string): Promise<number[]> {
    return this.embed(text);
  }

  private async drain(): Promise<void> {
    this.processing = true;
    while (this.queue.length > 0) {
      const entry = this.queue.shift()!;
      try {
        const vector = await this.callOllama(entry.text);
        entry.resolve(vector);
      } catch (err) {
        entry.reject(err);
      }
    }
    this.processing = false;
  }

  private async callOllama(text: string, attempt = 0): Promise<number[]> {
    if (!OLLAMA_URL) throw new Error('OLLAMA_URL not configured');
    const maxAttempts = 5;
    const start = Date.now();
    try {
      const res = await fetch(`${OLLAMA_URL}/api/embeddings`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ model: EMBEDDING_MODEL, prompt: text }),
        signal: AbortSignal.timeout(30_000),
      });
      if (!res.ok) {
        throw new Error(`Ollama HTTP ${res.status}: ${await res.text()}`);
      }
      const data = (await res.json()) as OllamaEmbedResponse;
      const elapsed = Date.now() - start;
      if (elapsed > 500) {
        logger.warn(`Embedding generation took ${elapsed}ms (>${500}ms threshold)`);
      } else {
        logger.debug(`Embedding generated in ${elapsed}ms`);
      }
      this.available = true;
      return data.embedding;
    } catch (err) {
      if (attempt >= maxAttempts - 1) {
        logger.error(`EmbeddingService: Ollama unreachable after ${maxAttempts} attempts — disabling RAG`);
        this.available = false;
        throw err;
      }
      const delay = Math.min(1000 * 2 ** attempt, 30_000);
      logger.warn(`EmbeddingService: attempt ${attempt + 1} failed, retrying in ${delay}ms`, err);
      await new Promise(r => setTimeout(r, delay));
      return this.callOllama(text, attempt + 1);
    }
  }

  /**
   * Warm up: verify Ollama is reachable at startup.
   * Skipped entirely when OLLAMA_URL is not configured.
   * Logs but does not throw.
   */
  async warmup(): Promise<void> {
    if (!OLLAMA_URL) {
      logger.info('EmbeddingService: OLLAMA_URL not set — embeddings disabled. Set OLLAMA_URL to enable RAG.');
      this.available = false;
      return;
    }
    try {
      await this.callOllama('warmup');
      logger.info(`EmbeddingService ready (model: ${EMBEDDING_MODEL}, url: ${OLLAMA_URL})`);
    } catch {
      logger.warn(`EmbeddingService: Ollama not reachable at ${OLLAMA_URL} — RAG disabled until Ollama comes online`);
      this.available = false;
    }
  }
}
