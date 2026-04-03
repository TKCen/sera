import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { ProviderHealthService } from './ProviderHealthService.js';
import type { ProviderConfig } from './ProviderRegistry.js';

// Mock Logger
vi.mock('../lib/logger.js', () => {
  return {
    Logger: class {
      info = vi.fn();
      error = vi.fn();
      warn = vi.fn();
      debug = vi.fn();
    },
  };
});

describe('ProviderHealthService', () => {
  let service: ProviderHealthService;
  const originalFetch = global.fetch;

  beforeEach(() => {
    service = new ProviderHealthService();
    global.fetch = vi.fn();
    vi.useFakeTimers();
  });

  afterEach(() => {
    global.fetch = originalFetch;
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  describe('checkHealth', () => {
    it('should probe and cache the result', async () => {
      const config: ProviderConfig = {
        modelName: 'gpt-4',
        api: 'openai-completions',
        provider: 'openai',
      };

      vi.mocked(global.fetch).mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => ({ data: [] }),
      } as Response);

      const status1 = await service.checkHealth(config);
      expect(status1.reachable).toBe(true);
      expect(global.fetch).toHaveBeenCalledTimes(1);

      // Second call should be cached
      const status2 = await service.checkHealth(config);
      expect(status2).toEqual(status1);
      expect(global.fetch).toHaveBeenCalledTimes(1);
    });

    it('should re-probe after TTL expires', async () => {
      const config: ProviderConfig = {
        modelName: 'gpt-4',
        api: 'openai-completions',
        provider: 'openai',
      };

      vi.mocked(global.fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ data: [] }),
      } as Response);

      await service.checkHealth(config);
      expect(global.fetch).toHaveBeenCalledTimes(1);

      // Advance time by 61 seconds (TTL is 60s)
      vi.advanceTimersByTime(61_000);

      await service.checkHealth(config);
      expect(global.fetch).toHaveBeenCalledTimes(2);
    });
  });

  describe('discoverModels', () => {
    it('should discover Google models', async () => {
      const config: ProviderConfig = {
        modelName: 'gemini-pro',
        api: 'openai-completions',
        provider: 'google',
        apiKey: 'test-key',
      };

      vi.mocked(global.fetch).mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          models: [
            { name: 'models/gemini-pro', supportedGenerationMethods: ['generateContent'] },
            { name: 'models/gemini-ultra', supportedGenerationMethods: ['other'] },
          ],
        }),
      } as Response);

      const models = await service.discoverModels(config);
      expect(models).toEqual(['gemini-pro']);
      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('generativelanguage.googleapis.com'),
        expect.anything()
      );
    });

    it('should discover Ollama models', async () => {
      const config: ProviderConfig = {
        modelName: 'llama3',
        api: 'openai-completions',
        provider: 'ollama',
        baseUrl: 'http://localhost:11434/v1',
      };

      vi.mocked(global.fetch).mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          models: [{ name: 'llama3:latest' }, { name: 'mistral:latest' }],
        }),
      } as Response);

      const models = await service.discoverModels(config);
      expect(models).toEqual(['llama3:latest', 'mistral:latest']);
      expect(global.fetch).toHaveBeenCalledWith('http://localhost:11434/api/tags', expect.anything());
    });

    it('should discover OpenAI-compatible models', async () => {
      const config: ProviderConfig = {
        modelName: 'custom',
        api: 'openai-completions',
        baseUrl: 'http://custom-provider/v1',
        apiKey: 'key',
      };

      vi.mocked(global.fetch).mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          data: [{ id: 'model-1' }, { id: 'model-2' }],
        }),
      } as Response);

      const models = await service.discoverModels(config);
      expect(models).toEqual(['model-1', 'model-2']);
      expect(global.fetch).toHaveBeenCalledWith('http://custom-provider/v1/models', expect.anything());
    });
  });

  describe('probe', () => {
    it('should probe Anthropic and handle success', async () => {
      const config: ProviderConfig = {
        modelName: 'claude-3',
        api: 'anthropic-messages',
        provider: 'anthropic',
      };

      vi.mocked(global.fetch).mockResolvedValueOnce({
        status: 200,
        ok: true,
      } as Response);

      const status = await service.checkHealth(config);
      expect(status.reachable).toBe(true);
      expect(global.fetch).toHaveBeenCalledWith(
        'https://api.anthropic.com/v1/messages',
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('should handle Anthropic auth failure', async () => {
      const config: ProviderConfig = {
        modelName: 'claude-3',
        api: 'anthropic-messages',
        provider: 'anthropic',
      };

      vi.mocked(global.fetch).mockResolvedValueOnce({
        status: 401,
        ok: false,
      } as Response);

      const status = await service.checkHealth(config);
      expect(status.reachable).toBe(false);
      expect(status.error).toBe('Authentication failed');
    });

    it('should handle fetch errors', async () => {
      const config: ProviderConfig = {
        modelName: 'gpt-4',
        api: 'openai-completions',
        provider: 'openai',
      };

      vi.mocked(global.fetch).mockRejectedValueOnce(new Error('Network error'));

      const status = await service.checkHealth(config);
      expect(status.reachable).toBe(false);
      expect(status.error).toBe('Network error');
    });
  });
});
