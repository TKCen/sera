import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { Mocked } from 'vitest';
import fs from 'fs';

// Mock DNS for async URL validation
vi.mock('node:dns/promises', () => ({
  lookup: vi.fn().mockResolvedValue({ address: '93.184.216.34' }), // example.com
}));
import { DynamicProviderManager } from './DynamicProviderManager.js';
import type { ProviderRegistry, DynamicProviderConfig } from './ProviderRegistry.js';

// Mock fs
vi.mock('fs', () => ({
  default: {
    existsSync: vi.fn(),
    readFileSync: vi.fn(),
    promises: {
      writeFile: vi.fn(),
    },
  },
}));

// Mock logger to avoid console spam
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

describe('DynamicProviderManager', () => {
  let mockRegistry: Mocked<ProviderRegistry>;
  const configPath = '/mock/config/path.json';

  beforeEach(() => {
    mockRegistry = {
      registerDynamicModels: vi.fn(),
      unregisterDynamicModels: vi.fn(),
    } as unknown as Mocked<ProviderRegistry>;

    vi.clearAllMocks();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  describe('constructor / loadSync', () => {
    it('should load config if file exists', () => {
      const mockData = {
        dynamicProviders: [
          {
            id: 'test-1',
            name: 'Test 1',
            type: 'lm-studio',
            baseUrl: 'http://test',
            enabled: true,
            intervalMs: 1000,
          },
        ],
      };
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(mockData));

      const manager = new DynamicProviderManager(mockRegistry, configPath);
      const providers = manager.listProviders();
      expect(providers).toHaveLength(1);
      expect(providers[0]?.id).toBe('test-1');
    });

    it('should handle missing file gracefully', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);

      const manager = new DynamicProviderManager(mockRegistry, configPath);
      expect(manager.listProviders()).toHaveLength(0);
    });

    it('should handle malformed JSON gracefully', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue('invalid json');

      const manager = new DynamicProviderManager(mockRegistry, configPath);
      expect(manager.listProviders()).toHaveLength(0);
    });
  });

  describe('testConnection', () => {
    it('should successfully fetch and parse models', async () => {
      global.fetch = vi.fn().mockResolvedValue({
        ok: true,
        json: vi.fn().mockResolvedValue({ data: [{ id: 'model-1' }, { id: 'model-2' }] }),
      } as unknown as Response);

      vi.mocked(fs.existsSync).mockReturnValue(false);
      const manager = new DynamicProviderManager(mockRegistry, configPath);

      const result = await manager.testConnection('http://localhost:1234/v1', 'test-key');

      expect(global.fetch).toHaveBeenCalledWith('http://localhost:1234/v1/models', {
        headers: { Authorization: 'Bearer test-key' },
      });
      expect(result.success).toBe(true);
      expect(result.models).toEqual(['model-1', 'model-2']);
      expect(result.error).toBeUndefined();
    });

    it('should handle trailing slash in baseUrl', async () => {
      global.fetch = vi.fn().mockResolvedValue({
        ok: true,
        json: vi.fn().mockResolvedValue({ data: [{ id: 'model-1' }] }),
      } as unknown as Response);

      vi.mocked(fs.existsSync).mockReturnValue(false);
      const manager = new DynamicProviderManager(mockRegistry, configPath);

      await manager.testConnection('http://localhost:1234/v1/', 'test-key');

      expect(global.fetch).toHaveBeenCalledWith('http://localhost:1234/v1/models', {
        headers: { Authorization: 'Bearer test-key' },
      });
    });

    it('should handle HTTP errors', async () => {
      global.fetch = vi.fn().mockResolvedValue({
        ok: false,
        status: 401,
        statusText: 'Unauthorized',
      } as unknown as Response);

      vi.mocked(fs.existsSync).mockReturnValue(false);
      const manager = new DynamicProviderManager(mockRegistry, configPath);

      const result = await manager.testConnection('http://localhost:1234/v1');

      expect(result.success).toBe(false);
      expect(result.models).toEqual([]);
      expect(result.error).toBe('HTTP 401: Unauthorized');
    });

    it('should handle network errors', async () => {
      global.fetch = vi.fn().mockRejectedValue(new Error('Network error'));

      vi.mocked(fs.existsSync).mockReturnValue(false);
      const manager = new DynamicProviderManager(mockRegistry, configPath);

      const result = await manager.testConnection('http://localhost:1234/v1');

      expect(result.success).toBe(false);
      expect(result.error).toBe('Network error');
    });
  });

  describe('addProvider', () => {
    it('should add provider, save to file, and start timer if enabled', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const manager = new DynamicProviderManager(mockRegistry, configPath);

      const testConnectionSpy = vi
        .spyOn(manager, 'testConnection')
        .mockResolvedValue({ success: true, models: ['m1'] });

      const config: DynamicProviderConfig = {
        id: 'new-id',
        name: 'New',
        type: 'lm-studio',
        baseUrl: 'http://localhost:1234/v1',
        enabled: true,
        intervalMs: 5000,
      };

      await manager.addProvider(config);

      expect(manager.listProviders()).toHaveLength(1);
      expect(fs.promises.writeFile).toHaveBeenCalled();

      // Wait for async checkProvider to finish (including async URL validation)
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();

      // Should call testConnection immediately
      expect(testConnectionSpy).toHaveBeenCalled();

      // Timer should be active and trigger after interval
      testConnectionSpy.mockClear();
      await vi.advanceTimersByTimeAsync(5000);
      expect(testConnectionSpy).toHaveBeenCalled();

      manager.stop();
    });

    it('should add provider and save, but not start timer if disabled', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const manager = new DynamicProviderManager(mockRegistry, configPath);

      const testConnectionSpy = vi.spyOn(manager, 'testConnection');

      const config: DynamicProviderConfig = {
        id: 'new-id',
        name: 'New',
        type: 'lm-studio',
        baseUrl: 'http://new',
        enabled: false,
        intervalMs: 5000,
      };

      await manager.addProvider(config);

      expect(manager.listProviders()).toHaveLength(1);
      expect(fs.promises.writeFile).toHaveBeenCalled();

      // Should not call testConnection
      expect(testConnectionSpy).not.toHaveBeenCalled();

      manager.stop();
    });
  });

  describe('start / stop', () => {
    it('should start polling for enabled providers and clear timers on stop', async () => {
      const mockData = {
        dynamicProviders: [
          {
            id: 't1',
            name: 'T1',
            type: 'lm-studio',
            baseUrl: 'http://localhost:1234/v1',
            enabled: true,
            intervalMs: 1000,
          },
          {
            id: 't2',
            name: 'T2',
            type: 'lm-studio',
            baseUrl: 'http://localhost:5678/v1',
            enabled: false,
            intervalMs: 1000,
          },
        ],
      };
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(mockData));

      const manager = new DynamicProviderManager(mockRegistry, configPath);
      const testConnectionSpy = vi
        .spyOn(manager, 'testConnection')
        .mockResolvedValue({ success: true, models: [] });

      await manager.start();

      // Wait for async checks (including async URL validation)
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();

      // Should check only t1 immediately
      expect(testConnectionSpy).toHaveBeenCalledTimes(1);
      expect(testConnectionSpy).toHaveBeenCalledWith('http://localhost:1234/v1', undefined);

      testConnectionSpy.mockClear();

      manager.stop();

      // Should not trigger after interval because it was stopped
      await vi.advanceTimersByTimeAsync(2000);
      expect(testConnectionSpy).not.toHaveBeenCalled();
    });
  });

  describe('listProviders', () => {
    it('should omit apiKey from the returned list', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const manager = new DynamicProviderManager(mockRegistry, configPath);

      await manager.addProvider({
        id: 't1',
        name: 'T1',
        type: 'lm-studio',
        baseUrl: 'http://t1',
        apiKey: 'secret-key',
        enabled: true,
        intervalMs: 1000,
      });

      const providers = manager.listProviders();
      expect(providers[0]).not.toHaveProperty('apiKey');
      expect(providers[0]?.id).toBe('t1');
    });
  });

  describe('removeProvider', () => {
    it('should remove provider, clear timer, call unregister, and save', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const manager = new DynamicProviderManager(mockRegistry, configPath);

      await manager.addProvider({
        id: 't1',
        name: 'T1',
        type: 'lm-studio',
        baseUrl: 'http://localhost:1234/v1',
        enabled: true,
        intervalMs: 1000,
      });

      expect(manager.listProviders()).toHaveLength(1);

      await manager.removeProvider('t1');

      expect(manager.listProviders()).toHaveLength(0);
      expect(mockRegistry.unregisterDynamicModels).toHaveBeenCalledWith('t1');
      expect(fs.promises.writeFile).toHaveBeenCalledTimes(2); // once on add, once on remove

      manager.stop();
    });
  });

  describe('checkProvider / getStatuses', () => {
    it('should update status to ok and register models on success', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const manager = new DynamicProviderManager(mockRegistry, configPath);

      vi.spyOn(manager, 'testConnection').mockResolvedValue({ success: true, models: ['m1'] });

      await manager.addProvider({
        id: 't1',
        name: 'T1',
        type: 'lm-studio',
        baseUrl: 'http://localhost:1234/v1',
        enabled: true,
        intervalMs: 1000,
      });

      // Let initial microtasks finish (including async URL validation)
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();

      const statuses = manager.getStatuses();
      expect(statuses).toHaveLength(1);
      expect(statuses[0]?.id).toBe('t1');
      expect(statuses[0]?.status).toBe('ok');
      expect(statuses[0]?.discoveredModels).toEqual(['m1']);

      expect(mockRegistry.registerDynamicModels).toHaveBeenCalledWith('t1', [
        expect.objectContaining({
          modelName: 'dp-t1-m1',
          api: 'openai-completions',
        }),
      ]);

      manager.stop();
    });

    it('should update status to error on failure', async () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const manager = new DynamicProviderManager(mockRegistry, configPath);

      vi.spyOn(manager, 'testConnection').mockResolvedValue({
        success: false,
        models: [],
        error: 'Failed',
      });

      await manager.addProvider({
        id: 't2',
        name: 'T2',
        type: 'lm-studio',
        baseUrl: 'http://localhost:1234/v1',
        enabled: true,
        intervalMs: 1000,
      });

      // Let initial microtasks finish (including async URL validation)
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();

      const statuses = manager.getStatuses();
      expect(statuses).toHaveLength(1);
      expect(statuses[0]?.id).toBe('t2');
      expect(statuses[0]?.status).toBe('error');
      expect(statuses[0]?.error).toBe('Failed');

      expect(mockRegistry.registerDynamicModels).not.toHaveBeenCalled();

      manager.stop();
    });
  });
});
