import { describe, it, expect, vi, beforeEach, afterEach, type Mocked } from 'vitest';
import { CleanupService } from './CleanupService.js';
import type { AgentRegistry } from './registry.service.js';
import type { SandboxManager } from '../sandbox/SandboxManager.js';
describe('CleanupService', () => {
  let cleanupService: CleanupService;
  let mockRegistry: Mocked<AgentRegistry>;
  let mockSandboxManager: Mocked<SandboxManager>;

  beforeEach(() => {
    vi.useFakeTimers();
    mockRegistry = {
      listInstances: vi.fn().mockResolvedValue([]),
      updateInstanceStatus: vi.fn(),
    } as unknown as Mocked<AgentRegistry>;

    mockSandboxManager = {
      teardown: vi.fn(),
    } as unknown as Mocked<SandboxManager>;

    cleanupService = new CleanupService();
    cleanupService.setRegistry(mockRegistry);
    cleanupService.setSandboxManager(mockSandboxManager);
  });

  afterEach(() => {
    cleanupService.stop();
    vi.useRealTimers();
  });

  it('should clean up stale stopped and errored instances', async () => {
    const now = Date.now();
    const retentionMs = 60 * 60 * 1000;
    const staleTime = new Date(now - retentionMs - 1000).toISOString();
    const freshTime = new Date(now - retentionMs + 1000).toISOString();

    const instances = [
      { id: 'stale-stopped', status: 'stopped', updated_at: staleTime },
      { id: 'fresh-stopped', status: 'stopped', updated_at: freshTime },
      { id: 'stale-error', status: 'error', updated_at: staleTime },
    ];

    mockRegistry.listInstances.mockImplementation(async (filters?: { status?: string }) => {
      if (filters?.status === 'stopped')
        return instances.filter((i) => i.status === 'stopped') as unknown as never;
      if (filters?.status === 'error')
        return instances.filter((i) => i.status === 'error') as unknown as never;
      return [];
    });

    mockSandboxManager.teardown.mockResolvedValue(undefined);

    await cleanupService.runCleanupJob();

    expect(mockSandboxManager.teardown).toHaveBeenCalledWith('stale-stopped');
    expect(mockSandboxManager.teardown).toHaveBeenCalledWith('stale-error');
    expect(mockSandboxManager.teardown).not.toHaveBeenCalledWith('fresh-stopped');
  });

  it('should enforce ephemeral TTLs', async () => {
    cleanupService.registerEphemeralTTL('agent-1', 1); // 1 minute TTL

    // Advance time by 30 seconds - not expired
    vi.advanceTimersByTime(30000);
    await cleanupService.runCleanupJob();
    expect(mockSandboxManager.teardown).not.toHaveBeenCalled();

    // Advance time by another 31 seconds - expired
    vi.advanceTimersByTime(31000);
    mockSandboxManager.teardown.mockResolvedValue(undefined);
    mockRegistry.updateInstanceStatus.mockResolvedValue({} as unknown as never);

    await cleanupService.runCleanupJob();
    expect(mockSandboxManager.teardown).toHaveBeenCalledWith('agent-1');
    expect(mockRegistry.updateInstanceStatus).toHaveBeenCalledWith('agent-1', 'error');
  });
});
