import { describe, it, expect, vi, beforeEach, afterEach, type Mocked } from 'vitest';
import { HeartbeatService } from './HeartbeatService.js';
import type { AgentRegistry } from './registry.service.js';
import type { IntercomService } from '../intercom/IntercomService.js';

describe('HeartbeatService', () => {
  let service: HeartbeatService;
  let mockRegistry: Mocked<AgentRegistry>;
  let mockIntercom: Mocked<IntercomService>;

  beforeEach(() => {
    vi.useFakeTimers();
    mockRegistry = {
      updateLastHeartbeat: vi.fn().mockResolvedValue(undefined),
      updateInstanceStatus: vi.fn().mockResolvedValue(undefined),
    } as unknown as Mocked<AgentRegistry>;
    mockIntercom = {
      publish: vi.fn().mockResolvedValue(undefined),
    } as unknown as Mocked<IntercomService>;
    service = new HeartbeatService();
  });

  afterEach(() => {
    service.stop();
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it('should register a heartbeat and update the registry', async () => {
    service.setRegistry(mockRegistry);
    await service.registerHeartbeat('agent-1');
    expect(mockRegistry.updateLastHeartbeat).toHaveBeenCalledWith('agent-1');
  });

  it('should detect and mark stale instances as unresponsive', async () => {
    service.setRegistry(mockRegistry);
    service.setIntercom(mockIntercom);

    // Register heartbeat at T=0
    await service.registerHeartbeat('agent-1');

    // Advance time by 130s (default stale is 120s)
    vi.advanceTimersByTime(130000);

    await service.checkStaleInstances();

    expect(mockRegistry.updateInstanceStatus).toHaveBeenCalledWith('agent-1', 'unresponsive');
    expect(mockIntercom.publish).toHaveBeenCalledWith(
      'system.agents',
      expect.objectContaining({
        type: 'unresponsive',
        agentId: 'agent-1',
      })
    );
  });

  it('should return unhealthy instances', async () => {
    // Agent 1 at T=0
    await service.registerHeartbeat('agent-1');

    // Advance 130s
    vi.advanceTimersByTime(130000);

    // Agent 2 at T=130s
    await service.registerHeartbeat('agent-2');

    const unhealthy = service.getUnhealthyInstances(120000);
    expect(unhealthy).toHaveLength(1);
    expect(unhealthy[0]!.instanceId).toBe('agent-1');
  });

  it('should remove heartbeats', async () => {
    await service.registerHeartbeat('agent-1');
    service.removeHeartbeat('agent-1');
    const unhealthy = service.getUnhealthyInstances(0);
    expect(unhealthy).toHaveLength(0);
  });

  it('should stop the interval', () => {
    const clearIntervalSpy = vi.spyOn(global, 'clearInterval');
    service.stop();
    expect(clearIntervalSpy).toHaveBeenCalled();
  });

  it('should handle checkStaleInstances through interval', async () => {
    service.setRegistry(mockRegistry);
    await service.registerHeartbeat('agent-1');

    // Advance 130s. The interval runs every 30s.
    // At 30, 60, 90, 120 the check will run.
    // At 120, it's exactly 120s since T=0, might not be > 120000 yet depending on exact Date.now()

    vi.advanceTimersByTime(130000);

    // We need to wait for any pending promises if checkStaleInstances is async
    // but here we just want to see if it was called.
    // Since it's called inside the interval:
    // setInterval(() => { this.checkStaleInstances()... }, 30000)

    // Triggering the intervals
    await vi.runOnlyPendingTimersAsync();

    expect(mockRegistry.updateInstanceStatus).toHaveBeenCalledWith('agent-1', 'unresponsive');
  });
});
