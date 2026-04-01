import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { EmbeddingRouter } from './embedding-router.js';
import type { EmbeddingConfig } from './embedding-config.js';

describe('EmbeddingRouter', () => {
  let mockFetch: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    mockFetch = vi.fn();
    vi.stubGlobal('fetch', mockFetch);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const baseOllamaConfig: EmbeddingConfig = {
    provider: 'ollama',
    model: 'nomic-embed-text',
    baseUrl: 'http://localhost:11434',
    dimension: 768,
  };

  const baseOpenAIConfig: EmbeddingConfig = {
    provider: 'openai',
    model: 'text-embedding-3-small',
    baseUrl: 'https://api.openai.com',
    apiKey: 'test-key',
    dimension: 1536,
  };

  describe('constructor and config management', () => {
    it('returns config and dimension correctly', () => {
      const router = new EmbeddingRouter(baseOllamaConfig);
      expect(router.getConfig()).toEqual(baseOllamaConfig);
      expect(router.getDimension()).toBe(768);
    });

    it('updates config and dimension correctly', () => {
      const router = new EmbeddingRouter(baseOllamaConfig);
      router.updateConfig(baseOpenAIConfig);
      expect(router.getConfig()).toEqual(baseOpenAIConfig);
      expect(router.getDimension()).toBe(1536);
    });

    it('returns true for isConfigured if baseUrl exists', () => {
      const router = new EmbeddingRouter(baseOllamaConfig);
      expect(router.isConfigured()).toBe(true);
    });

    it('returns false for isConfigured if baseUrl is empty', () => {
      const router = new EmbeddingRouter({ ...baseOllamaConfig, baseUrl: '' });
      expect(router.isConfigured()).toBe(false);
    });
  });

  describe('embed()', () => {
    it('throws error if baseUrl is missing', async () => {
      const router = new EmbeddingRouter({ ...baseOllamaConfig, baseUrl: '' });
      await expect(router.embed('hello')).rejects.toThrow('EmbeddingRouter: no baseUrl configured');
    });

    it('calls Ollama API correctly', async () => {
      const router = new EmbeddingRouter(baseOllamaConfig);
      const mockEmbedding = [0.1, 0.2, 0.3];
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ embedding: mockEmbedding }),
      } as unknown as Response);

      const result = await router.embed('test text');

      expect(result).toEqual(mockEmbedding);
      expect(mockFetch).toHaveBeenCalledTimes(1);
      expect(mockFetch).toHaveBeenCalledWith(
        'http://localhost:11434/api/embeddings',
        expect.objectContaining({
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ model: 'nomic-embed-text', prompt: 'test text' }),
        })
      );
    });

    it('throws error if Ollama API fails', async () => {
      const router = new EmbeddingRouter(baseOllamaConfig);
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 500,
        text: async () => 'Internal Server Error',
      } as unknown as Response);

      await expect(router.embed('test text')).rejects.toThrow(
        'Ollama HTTP 500: Internal Server Error'
      );
    });

    it('calls OpenAI API correctly', async () => {
      const router = new EmbeddingRouter(baseOpenAIConfig);
      const mockEmbedding = [0.4, 0.5, 0.6];
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ data: [{ embedding: mockEmbedding }] }),
      } as unknown as Response);

      const result = await router.embed('test text');

      expect(result).toEqual(mockEmbedding);
      expect(mockFetch).toHaveBeenCalledTimes(1);
      expect(mockFetch).toHaveBeenCalledWith(
        'https://api.openai.com/v1/embeddings',
        expect.objectContaining({
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            Authorization: 'Bearer test-key',
          },
          body: JSON.stringify({ model: 'text-embedding-3-small', input: 'test text' }),
        })
      );
    });

    it('calls OpenAI API with env var API key if apiKey EnvVar is set', async () => {
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      const { apiKey: _apiKey, ...restOpenAIConfig } = baseOpenAIConfig;
      const config: EmbeddingConfig = {
        ...restOpenAIConfig,
        apiKeyEnvVar: 'CUSTOM_OPENAI_KEY',
      };
      process.env.CUSTOM_OPENAI_KEY = 'env-test-key';

      const router = new EmbeddingRouter(config);
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ data: [{ embedding: [0.1] }] }),
      } as unknown as Response);

      await router.embed('test text');

      expect(mockFetch).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({
          headers: expect.objectContaining({
            Authorization: 'Bearer env-test-key',
          }),
        })
      );

      delete process.env.CUSTOM_OPENAI_KEY;
    });

    it('throws error if OpenAI API fails', async () => {
      const router = new EmbeddingRouter(baseOpenAIConfig);
      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 401,
        text: async () => 'Unauthorized',
      } as unknown as Response);

      await expect(router.embed('test text')).rejects.toThrow(
        'Embedding API HTTP 401: Unauthorized'
      );
    });

    it('throws error if OpenAI API returns invalid format', async () => {
      const router = new EmbeddingRouter(baseOpenAIConfig);
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ data: [] }), // missing data[0].embedding
      } as unknown as Response);

      await expect(router.embed('test text')).rejects.toThrow(
        'Unexpected response format — missing data[0].embedding'
      );
    });
  });

  describe('testConnection()', () => {
    it('returns ok: true on successful connection', async () => {
      const router = new EmbeddingRouter(baseOllamaConfig);
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ embedding: [0.1, 0.2] }),
      } as unknown as Response);

      const result = await router.testConnection();

      expect(result.ok).toBe(true);
      expect(result.latencyMs).toBeGreaterThanOrEqual(0);
      expect(result.dimension).toBe(2);
      expect(result.error).toBeUndefined();
    });

    it('returns ok: false on failed connection', async () => {
      const router = new EmbeddingRouter(baseOllamaConfig);
      mockFetch.mockRejectedValueOnce(new Error('Network error'));

      const result = await router.testConnection();

      expect(result.ok).toBe(false);
      expect(result.latencyMs).toBeGreaterThanOrEqual(0);
      expect(result.dimension).toBeUndefined();
      expect(result.error).toBe('Network error');
    });
  });
});
