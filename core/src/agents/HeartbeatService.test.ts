import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { HeartbeatService } from './HeartbeatService.js';
import { AgentRegistry } from './registry.service.js';
import { IntercomService } from '../intercom/IntercomService.js';

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

// Mock dependencies
vi.mock('./registry.service.js', () => {
  class Mock {
    updateLastHeartbeat = vi.fn();
    updateInstanceStatus = vi.fn();
  }
  return { AgentRegistry: Mock };
});

vi.mock('../intercom/IntercomService.js', () => {
  class Mock {
    publish = vi.fn().mockResolvedValue(undefined);
  }
  return { IntercomService: Mock };
});

describe('HeartbeatService', () => {
  let service: HeartbeatService;
  let registry: AgentRegistry;
  let intercom: IntercomService;

  beforeEach(() => {
    vi.useFakeTimers();
    service = new HeartbeatService();
    registry = new AgentRegistry({} as any);
    intercom = new IntercomService({} as any);
    service.setRegistry(registry);
    service.setIntercom(intercom);
  });

  afterEach(() => {
    service.stop();
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  describe('registerHeartbeat', () => {
    it('should update last heartbeat in internal map and registry', async () => {
      const instanceId = 'inst-1';
      await service.registerHeartbeat(instanceId);

      expect(registry.updateLastHeartbeat).toHaveBeenCalledWith(instanceId);
      const unhealthy = service.getUnhealthyInstances(60000);
      expect(unhealthy.find((u) => u.instanceId === instanceId)).toBeUndefined();
    });
  });

  describe('checkStaleInstances', () => {
    it('should identify stale instances and mark them unresponsive', async () => {
      const instanceId = 'inst-1';
      await service.registerHeartbeat(instanceId);

      // Advance time beyond HEARTBEAT_STALE_MS (120000)
      vi.advanceTimersByTime(121000);

      await service.checkStaleInstances();

      expect(registry.updateInstanceStatus).toHaveBeenCalledWith(instanceId, 'unresponsive');
      expect(intercom.publish).toHaveBeenCalledWith('system.agents', expect.objectContaining({
        type: 'unresponsive',
        agentId: instanceId,
      }));
    });

    it('should not mark instances unresponsive if they are within time limit', async () => {
      const instanceId = 'inst-1';
      await service.registerHeartbeat(instanceId);

      // Advance time but not beyond limit
      vi.advanceTimersByTime(60000);

      await service.checkStaleInstances();

      expect(registry.updateInstanceStatus).not.toHaveBeenCalled();
    });
  });

  describe('getUnhealthyInstances', () => {
    it('should return list of unhealthy instances', async () => {
      const instanceId = 'inst-1';
      await service.registerHeartbeat(instanceId);

      vi.advanceTimersByTime(121000);

      const unhealthy = service.getUnhealthyInstances();
      expect(unhealthy).toHaveLength(1);
      expect(unhealthy[0]!.instanceId).toBe(instanceId);
    });
  });

  describe('removeHeartbeat', () => {
    it('should stop tracking instance heartbeat', async () => {
      const instanceId = 'inst-1';
      await service.registerHeartbeat(instanceId);

      service.removeHeartbeat(instanceId);

      vi.advanceTimersByTime(121000);
      await service.checkStaleInstances();

      expect(registry.updateInstanceStatus).not.toHaveBeenCalled();
    });
  });
});
