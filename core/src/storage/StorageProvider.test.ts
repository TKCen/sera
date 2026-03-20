import { describe, it, expect, vi, beforeEach } from 'vitest';
import { StorageProviderFactory } from './StorageProvider.js';
import { LocalStorageProvider } from './LocalStorageProvider.js';
import { DockerVolumeProvider } from './DockerVolumeProvider.js';

// ── Mock Docker for DockerVolumeProvider ────────────────────────────────────────

function createMockDocker() {
  const mockVolume = {
    remove: vi.fn().mockResolvedValue(undefined),
  };

  return {
    createVolume: vi.fn().mockResolvedValue(mockVolume),
    getVolume: vi.fn().mockReturnValue(mockVolume),
    _volume: mockVolume,
  };
}

// ── LocalStorageProvider ────────────────────────────────────────────────────────

describe('LocalStorageProvider', () => {
  let provider: LocalStorageProvider;

  beforeEach(() => {
    provider = new LocalStorageProvider('/workspaces');
  });

  it('should have name "local"', () => {
    expect(provider.name).toBe('local');
  });

  it('should return a bind mount result on mount', async () => {
    const result = await provider.mount('test-agent');
    expect(result.hostPathOrVolume).toBe('/workspaces/test-agent');
    expect(result.isVolume).toBe(false);
  });

  it('should use custom workspace path when provided', async () => {
    const result = await provider.mount('test-agent', '/custom/path');
    expect(result.hostPathOrVolume).toBe('/custom/path');
  });

  it('should return the host path from getPath', () => {
    expect(provider.getPath('my-agent')).toBe('/workspaces/my-agent');
    expect(provider.getPath('my-agent', '/override')).toBe('/override');
  });

  it('should build correct bind mount strings', () => {
    expect(provider.getBindMount('agent-a', '/workspace', 'rw')).toBe(
      '/workspaces/agent-a:/workspace:rw'
    );

    expect(provider.getBindMount('agent-b', '/workspace', 'ro')).toBe(
      '/workspaces/agent-b:/workspace:ro'
    );
  });

  it('should use custom workspace path in bind mount', () => {
    expect(provider.getBindMount('agent-a', '/workspace', 'rw', '/my/custom/path')).toBe(
      '/my/custom/path:/workspace:rw'
    );
  });

  it('should be a no-op on unmount', async () => {
    // Should not throw
    await provider.unmount('agent-a');
  });
});

// ── DockerVolumeProvider ────────────────────────────────────────────────────────

describe('DockerVolumeProvider', () => {
  let provider: DockerVolumeProvider;
  let mockDocker: ReturnType<typeof createMockDocker>;

  beforeEach(() => {
    mockDocker = createMockDocker();
    provider = new DockerVolumeProvider(mockDocker as any, 'sera-ws');
  });

  it('should have name "docker-volume"', () => {
    expect(provider.name).toBe('docker-volume');
  });

  it('should create a Docker volume on mount', async () => {
    const result = await provider.mount('test-agent');

    expect(result.hostPathOrVolume).toBe('sera-ws-test-agent');
    expect(result.isVolume).toBe(true);
    expect(mockDocker.createVolume).toHaveBeenCalledWith({
      Name: 'sera-ws-test-agent',
      Labels: {
        'sera.storage': 'true',
        'sera.agent': 'test-agent',
        'sera.provider': 'docker-volume',
      },
    });
  });

  it('should remove the volume on unmount', async () => {
    await provider.unmount('test-agent');

    expect(mockDocker.getVolume).toHaveBeenCalledWith('sera-ws-test-agent');
    expect(mockDocker._volume.remove).toHaveBeenCalledOnce();
  });

  it('should not throw if volume does not exist on unmount', async () => {
    mockDocker._volume.remove.mockRejectedValue(new Error('No such volume'));
    await expect(provider.unmount('missing-agent')).resolves.toBeUndefined();
  });

  it('should return volume name from getPath', () => {
    expect(provider.getPath('test-agent')).toBe('sera-ws-test-agent');
  });

  it('should build volume mount strings', () => {
    expect(provider.getBindMount('agent-a', '/workspace', 'rw')).toBe(
      'sera-ws-agent-a:/workspace:rw'
    );

    expect(provider.getBindMount('agent-b', '/data', 'ro')).toBe('sera-ws-agent-b:/data:ro');
  });
});

// ── StorageProviderFactory ──────────────────────────────────────────────────────

describe('StorageProviderFactory', () => {
  it('should resolve a registered provider', () => {
    const factory = new StorageProviderFactory('local');
    factory.register(new LocalStorageProvider());

    const provider = factory.getProvider('local');
    expect(provider.name).toBe('local');
  });

  it('should fall back to the default provider when name is undefined', () => {
    const factory = new StorageProviderFactory('local');
    factory.register(new LocalStorageProvider());

    const provider = factory.getProvider(undefined);
    expect(provider.name).toBe('local');
  });

  it('should throw for an unregistered provider', () => {
    const factory = new StorageProviderFactory('local');
    expect(() => factory.getProvider('nfs')).toThrow(/not registered/);
  });

  it('should list all registered providers', () => {
    const factory = new StorageProviderFactory('local');
    factory.register(new LocalStorageProvider());
    factory.register(new DockerVolumeProvider(createMockDocker() as any));

    const names = factory.listProviders();
    expect(names).toContain('local');
    expect(names).toContain('docker-volume');
    expect(names).toHaveLength(2);
  });

  it('should support registering multiple providers and resolving each', () => {
    const factory = new StorageProviderFactory('local');
    factory.register(new LocalStorageProvider());
    factory.register(new DockerVolumeProvider(createMockDocker() as any));

    expect(factory.getProvider('local').name).toBe('local');
    expect(factory.getProvider('docker-volume').name).toBe('docker-volume');
  });
});
