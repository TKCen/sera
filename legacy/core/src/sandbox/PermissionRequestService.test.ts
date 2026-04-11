import { describe, it, expect, vi, beforeEach } from 'vitest';
import { PermissionRequestService } from './PermissionRequestService.js';
import { AgentRegistry } from '../agents/registry.service.js';
import { IntercomService } from '../intercom/IntercomService.js';

describe('PermissionRequestService', () => {
  let service: PermissionRequestService;
  let mockRegistry: any;
  let mockIntercom: any;

  // Use valid UUIDs to avoid AuditService validation errors
  const agentId = '00000000-0000-0000-0000-000000000001';
  const requestId = '00000000-0000-0000-0000-000000000002';

  beforeEach(() => {
    mockRegistry = {
      createPermissionGrant: vi.fn(),
      listActivePermissionGrants: vi.fn().mockResolvedValue([]),
    };
    mockIntercom = {
      publishSystemEvent: vi.fn().mockResolvedValue(undefined),
    };
    service = new PermissionRequestService(
      mockRegistry as unknown as AgentRegistry,
      mockIntercom as unknown as IntercomService
    );
  });

  describe('initialize', () => {
    it('should hydrate persistent grants from the registry', async () => {
      const mockGrants = [
        {
          agent_instance_id: agentId,
          grant_type: 'persistent',
          resource_type: 'filesystem',
          resource_value: '/tmp/foo',
        },
      ];
      mockRegistry.listActivePermissionGrants.mockResolvedValue(mockGrants);

      await service.initialize();

      expect(mockRegistry.listActivePermissionGrants).toHaveBeenCalled();
      expect(service.hasActiveGrant(agentId, 'filesystem', '/tmp/foo')).toBe(true);
    });
  });

  describe('decide', () => {
    it('should store persistent grants in the registry and in-memory map', async () => {
      // Setup pending request
      (service as any).pending.set(requestId, {
        requestId,
        agentId,
        agentName: 'TestAgent',
        dimension: 'filesystem',
        value: '/data',
        status: 'pending',
      });

      const decision = { decision: 'grant', grantType: 'persistent' };
      mockRegistry.createPermissionGrant.mockResolvedValue({
        agent_instance_id: agentId,
        grant_type: 'persistent',
        resource_type: 'filesystem',
        resource_value: '/data',
      });

      await service.decide(requestId, decision as any);

      expect(mockRegistry.createPermissionGrant).toHaveBeenCalledWith({
        agentInstanceId: agentId,
        grantType: 'persistent',
        resourceType: 'filesystem',
        resourceValue: '/data',
        mode: 'ro',
        approvedBy: undefined,
        expiresAt: undefined,
      });
      expect(service.hasActiveGrant(agentId, 'filesystem', '/data')).toBe(true);
    });

    it('should store session grants only in-memory', async () => {
      (service as any).pending.set(requestId, {
        requestId,
        agentId,
        agentName: 'TestAgent',
        dimension: 'network',
        value: 'google.com',
        status: 'pending',
      });

      const decision = { decision: 'grant', grantType: 'session' };

      await service.decide(requestId, decision as any);

      expect(mockRegistry.createPermissionGrant).not.toHaveBeenCalled();
      expect(service.hasActiveGrant(agentId, 'network', 'google.com')).toBe(true);
    });
  });

  describe('hasActiveGrant', () => {
    it('should support prefix matching for filesystem paths', async () => {
      const mockGrants = [
        {
          agent_instance_id: agentId,
          grant_type: 'persistent',
          resource_type: 'filesystem',
          resource_value: '/var/log',
        },
      ];
      mockRegistry.listActivePermissionGrants.mockResolvedValue(mockGrants);
      await service.initialize();

      expect(service.hasActiveGrant(agentId, 'filesystem', '/var/log/syslog')).toBe(true);
      expect(service.hasActiveGrant(agentId, 'filesystem', '/var/lib')).toBe(false);
    });
  });
});
