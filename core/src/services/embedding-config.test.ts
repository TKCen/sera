import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import * as fs from 'node:fs';
import {
  loadEmbeddingConfig,
  saveEmbeddingConfig,
  maskConfig,
  resolveApiKey,
  KNOWN_EMBEDDING_MODELS,
} from './embedding-config.js';

vi.mock('node:fs', () => ({
  readFileSync: vi.fn(),
  writeFileSync: vi.fn(),
  existsSync: vi.fn(),
}));

vi.mock('../lib/logger.js', () => ({
  Logger: class {
    info = vi.fn();
    warn = vi.fn();
    error = vi.fn();
    debug = vi.fn();
  },
}));

describe('embedding-config', () => {
  const originalEnv = process.env;

  beforeEach(() => {
    vi.clearAllMocks();
    process.env = { ...originalEnv };
    delete process.env.EMBEDDING_CONFIG_PATH;
    delete process.env.OLLAMA_URL;
    delete process.env.EMBEDDING_MODEL;
    delete process.env.OPENAI_API_KEY;
  });

  afterEach(() => {
    process.env = originalEnv;
  });

  describe('loadEmbeddingConfig', () => {
    it('loads from config file if it exists', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      const mockConfig = {
        provider: 'openai',
        model: 'text-embedding-3-small',
        baseUrl: 'https://api.openai.com/v1',
        dimension: 1536,
      };
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(mockConfig));

      const config = loadEmbeddingConfig();
      expect(fs.existsSync).toHaveBeenCalled();
      expect(fs.readFileSync).toHaveBeenCalled();
      expect(config).toEqual(mockConfig);
    });

    it('falls back to env vars if config file is invalid JSON', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue('invalid json');
      process.env.OLLAMA_URL = 'http://localhost:11434';
      process.env.EMBEDDING_MODEL = 'nomic-embed-text';

      const config = loadEmbeddingConfig();
      expect(config).toEqual({
        provider: 'ollama',
        model: 'nomic-embed-text',
        baseUrl: 'http://localhost:11434',
        dimension: 768, // fallback from known models
      });
    });

    it('falls back to env vars if no config file exists', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      process.env.OLLAMA_URL = 'http://remote:11434';
      process.env.EMBEDDING_MODEL = 'all-minilm';

      const config = loadEmbeddingConfig();
      expect(config).toEqual({
        provider: 'ollama',
        model: 'all-minilm',
        baseUrl: 'http://remote:11434',
        dimension: 384, // From KNOWN_EMBEDDING_MODELS
      });
    });

    it('returns unconfigured default if neither file nor env vars exist', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);

      const config = loadEmbeddingConfig();
      expect(config).toEqual({
        provider: 'ollama',
        model: 'nomic-embed-text', // default model
        baseUrl: '',
        dimension: 768,
      });
    });
  });

  describe('saveEmbeddingConfig', () => {
    it('writes the config to disk', () => {
      const mockConfig = {
        provider: 'openai' as const,
        model: 'text-embedding-3-large',
        baseUrl: 'https://api.openai.com/v1',
        dimension: 3072,
        apiKeyEnvVar: 'MY_CUSTOM_KEY',
      };

      saveEmbeddingConfig(mockConfig);
      expect(fs.writeFileSync).toHaveBeenCalledWith(
        '/app/config/embedding.json',
        JSON.stringify(mockConfig, null, 2),
        'utf-8'
      );
    });
  });

  describe('maskConfig', () => {
    it('masks apiKey keeping first 4 and last 4 characters', () => {
      const config = {
        provider: 'openai' as const,
        model: 'text-embedding-3-small',
        baseUrl: 'url',
        dimension: 1536,
        apiKey: 'sk-1234567890abcdefghijklmnopqrstuvwxyz9876',
      };

      const masked = maskConfig(config);
      expect(masked.apiKey).toBe('sk-1…9876');
      expect(masked.provider).toBe('openai');
    });

    it('does nothing if apiKey is undefined', () => {
      const config = {
        provider: 'ollama' as const,
        model: 'nomic-embed-text',
        baseUrl: 'url',
        dimension: 768,
      };

      const masked = maskConfig(config);
      expect(masked).toEqual(config);
    });
  });

  describe('resolveApiKey', () => {
    it('returns explicit apiKey if provided', () => {
      const config = {
        provider: 'openai' as const,
        model: 'm',
        baseUrl: 'u',
        dimension: 1,
        apiKey: 'literal-key',
        apiKeyEnvVar: 'ENV_VAR_KEY',
      };
      process.env.ENV_VAR_KEY = 'from-env-var';

      expect(resolveApiKey(config)).toBe('literal-key');
    });

    it('resolves from apiKeyEnvVar if literal key not provided', () => {
      const config = {
        provider: 'openai' as const,
        model: 'm',
        baseUrl: 'u',
        dimension: 1,
        apiKeyEnvVar: 'MY_ENV_VAR',
      };
      process.env.MY_ENV_VAR = 'resolved-key';

      expect(resolveApiKey(config)).toBe('resolved-key');
    });

    it('falls back to OPENAI_API_KEY for openai provider', () => {
      const config = {
        provider: 'openai' as const,
        model: 'm',
        baseUrl: 'u',
        dimension: 1,
      };
      process.env.OPENAI_API_KEY = 'openai-fallback-key';

      expect(resolveApiKey(config)).toBe('openai-fallback-key');
    });

    it('returns undefined if no key resolves', () => {
      const config = {
        provider: 'ollama' as const,
        model: 'm',
        baseUrl: 'u',
        dimension: 1,
      };

      expect(resolveApiKey(config)).toBeUndefined();
    });
  });
});
