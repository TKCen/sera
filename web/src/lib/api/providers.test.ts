import { describe, it, expect, vi, beforeEach } from 'vitest';
import { testLLMConfig, getLLMConfig, updateLLMConfig } from './providers';
import { request } from './client';
import type { LLMConfig } from './types';

vi.mock('./client', () => ({
  request: vi.fn(),
}));

describe('providers api', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('testLLMConfig', () => {
    it('should call request with correct parameters for success', async () => {
      const mockResponse = { success: true, model: 'gpt-4o', response: 'Connected' };
      vi.mocked(request).mockResolvedValueOnce(mockResponse);

      const result = await testLLMConfig();

      expect(request).toHaveBeenCalledWith('/config/llm/test', {
        method: 'POST',
      });
      expect(result).toEqual(mockResponse);
    });

    it('should handle failure responses', async () => {
      const mockResponse = { success: false, error: 'Invalid API key' };
      vi.mocked(request).mockResolvedValueOnce(mockResponse);

      const result = await testLLMConfig();

      expect(result.success).toBe(false);
      expect(result.error).toBe('Invalid API key');
    });
  });

  describe('getLLMConfig', () => {
    it('should fetch LLM configuration', async () => {
      const mockConfig: LLMConfig = {
        baseUrl: 'http://localhost:11434',
        model: 'llama3',
      };
      vi.mocked(request).mockResolvedValueOnce(mockConfig);

      const result = await getLLMConfig();

      expect(request).toHaveBeenCalledWith('/config/llm');
      expect(result).toEqual(mockConfig);
    });
  });

  describe('updateLLMConfig', () => {
    it('should update LLM configuration via POST', async () => {
      const config: LLMConfig = {
        baseUrl: 'https://api.openai.com/v1',
        apiKey: 'test-key',
        model: 'gpt-4o',
      };
      const mockResponse = { success: true };
      vi.mocked(request).mockResolvedValueOnce(mockResponse);

      const result = await updateLLMConfig(config);

      expect(request).toHaveBeenCalledWith('/config/llm', {
        method: 'POST',
        body: JSON.stringify(config),
      });
      expect(result).toEqual(mockResponse);
    });
  });
});
