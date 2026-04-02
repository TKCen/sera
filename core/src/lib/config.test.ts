import { describe, it, expect, vi, beforeEach } from 'vitest';
import fs from 'fs';
import path from 'path';
import type { config as ConfigType } from './config.js';

vi.mock('fs');
vi.mock('path', async (importOriginal) => {
  const original = (await importOriginal()) as typeof path;
  return {
    ...original,
    join: vi.fn((...args: string[]) => args.join('/')),
    dirname: vi.fn((p: string) => p.split('/').slice(0, -1).join('/')),
  };
});

describe('config', () => {
  let config: typeof ConfigType;

  beforeEach(async () => {
    vi.clearAllMocks();
    vi.resetModules();
    // Dynamically import config to ensure fresh module state for each test
    const mod = (await import('./config.js')) as unknown as {
      config: typeof ConfigType;
    };
    config = mod.config;
  });

  describe('llm property (Legacy Fallback)', () => {
    it('returns default legacy config when no files exist', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const llm = config.llm;
      // We check against the default values set at module load time
      expect(llm.baseUrl).toBeDefined();
      expect(llm.apiKey).toBeDefined();
      expect(llm.model).toBeDefined();
    });

    it('loads legacy config from llm.json', () => {
      vi.mocked(fs.existsSync).mockImplementation((p) => (p as string).includes('llm.json'));
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({
          baseUrl: 'http://custom:1234/v1',
          model: 'custom-model',
        })
      );

      const llm = config.llm;
      expect(llm.baseUrl).toBe('http://custom:1234/v1');
      expect(llm.model).toBe('custom-model');
    });

    it('derives llm config from active provider in providers.json', () => {
      vi.mocked(fs.existsSync).mockImplementation((p) => (p as string).includes('providers.json'));
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({
          activeProvider: 'openai',
          providers: {
            openai: {
              baseUrl: 'https://api.openai.com/v1',
              apiKey: 'sk-123',
              model: 'gpt-4o',
            },
          },
        })
      );

      const llm = config.llm;
      expect(llm.baseUrl).toBe('https://api.openai.com/v1');
      expect(llm.apiKey).toBe('sk-123');
      expect(llm.model).toBe('gpt-4o');
    });
  });

  describe('providers property', () => {
    it('returns default providers config when providers.json missing', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const providers = config.providers;
      expect(providers.activeProvider).toBe('lmstudio');
      expect(providers.providers).toEqual({});
    });

    it('loads providers config from file', () => {
      vi.mocked(fs.existsSync).mockImplementation((p) => (p as string).includes('providers.json'));
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({
          activeProvider: 'ollama',
          providers: {
            ollama: { baseUrl: 'http://localhost:11434/v1', model: 'llama3' },
          },
        })
      );

      const providers = config.providers;
      expect(providers.activeProvider).toBe('ollama');
      // @ts-expect-error - testing dynamic properties
      expect(providers.providers['ollama']?.model).toBe('llama3');
    });
  });

  describe('saveProviderConfig', () => {
    it('updates and saves the provider config', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({
          activeProvider: 'lmstudio',
          providers: {},
        })
      );

      config.saveProviderConfig('new-p', { baseUrl: 'url', apiKey: 'key', model: 'm' });

      expect(fs.writeFileSync).toHaveBeenCalledWith(
        expect.stringContaining('providers.json'),
        expect.stringContaining('"new-p"')
      );
    });
  });

  describe('setActiveProvider', () => {
    it('updates and saves the active provider', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({
          activeProvider: 'lmstudio',
          providers: {},
        })
      );

      config.setActiveProvider('openai');

      expect(fs.writeFileSync).toHaveBeenCalledWith(
        expect.stringContaining('providers.json'),
        expect.stringContaining('"activeProvider": "openai"')
      );
    });
  });

  describe('channels config', () => {
    it('has the expected structure', () => {
      expect(config.channels.telegram).toBeDefined();
      expect(config.channels.rateLimit.windowMs).toBeTypeOf('number');
      expect(config.channels.rateLimit.maxMessages).toBeTypeOf('number');
    });
  });
});
