import { describe, it, expect, vi, beforeEach } from 'vitest';
import fs from 'fs';
import path from 'path';
import { EgressAclManager } from './EgressAclManager.js';

vi.mock('fs');

// ── Mock Docker ─────────────────────────────────────────────────────────────────

function createMockDocker() {
  const mockExecInstance = {
    start: vi.fn().mockResolvedValue(undefined),
  };

  const mockContainer = {
    exec: vi.fn().mockResolvedValue(mockExecInstance),
  };

  return {
    getContainer: vi.fn().mockReturnValue(mockContainer),
    _container: mockContainer,
    _exec: mockExecInstance,
  };
}

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('EgressAclManager', () => {
  let mockDocker: ReturnType<typeof createMockDocker>;
  let manager: EgressAclManager;
  const aclDir = '/egress/acls';

  beforeEach(() => {
    vi.clearAllMocks();
    mockDocker = createMockDocker();
    manager = new EgressAclManager(
      mockDocker as unknown as import('dockerode'),
      aclDir,
      'sera-egress-proxy'
    );
    vi.mocked(fs.writeFileSync).mockImplementation(() => undefined);
    vi.mocked(fs.unlinkSync).mockImplementation(() => undefined);
    vi.mocked(fs.existsSync).mockReturnValue(false);
  });

  /** Find a writeFileSync call whose path contains the given substring */
  function findWriteCall(substr: string) {
    return vi
      .mocked(fs.writeFileSync)
      .mock.calls.find((c) => String(c[0]).replace(/\\/g, '/').includes(substr));
  }

  /** Find the last master include write */
  function findMasterWrite() {
    const calls = vi
      .mocked(fs.writeFileSync)
      .mock.calls.filter((c) => String(c[0]).replace(/\\/g, '/').endsWith('agents.conf'));
    return calls[calls.length - 1];
  }

  describe('onSpawn', () => {
    it('should write a per-agent ACL file with specific domains', async () => {
      await manager.onSpawn('agent-abc', '172.19.0.5', {
        outbound: ['github.com', 'api.openai.com'],
      });

      const agentAclCall = findWriteCall('agent-agent-abc.conf');
      expect(agentAclCall).toBeDefined();

      const content = String(agentAclCall![1]);
      expect(content).toContain('acl agent_172_19_0_5 src 172.19.0.5/32');
      expect(content).toContain('acl agent_172_19_0_5_domains dstdomain github.com api.openai.com');
      expect(content).toContain('http_access allow agent_172_19_0_5 agent_172_19_0_5_domains');
      expect(content).toContain('http_access deny agent_172_19_0_5');
    });

    it('should write a wildcard allow ACL for outbound: ["*"]', async () => {
      await manager.onSpawn('agent-wild', '172.19.0.6', {
        outbound: ['*'],
      });

      const agentAclCall = findWriteCall('agent-agent-wild.conf');
      expect(agentAclCall).toBeDefined();

      const content = String(agentAclCall![1]);
      expect(content).toContain('http_access allow agent_172_19_0_6');
      expect(content).not.toContain('dstdomain');
    });

    it('should skip ACL generation for empty outbound', async () => {
      await manager.onSpawn('agent-none', '172.19.0.7', {
        outbound: [],
      });

      expect(vi.mocked(fs.writeFileSync)).not.toHaveBeenCalled();
      expect(manager.activeCount).toBe(0);
    });

    it('should regenerate the master include on spawn', async () => {
      await manager.onSpawn('agent-a', '172.19.0.5', {
        outbound: ['github.com'],
      });

      const masterCall = findMasterWrite();
      expect(masterCall).toBeDefined();

      const masterContent = String(masterCall![1]);
      expect(masterContent).toContain('agent-agent-a.conf');
    });

    it('should trigger squid -k reconfigure', async () => {
      await manager.onSpawn('agent-reload', '172.19.0.8', {
        outbound: ['example.com'],
      });

      expect(mockDocker.getContainer).toHaveBeenCalledWith('sera-egress-proxy');
      expect(mockDocker._container.exec).toHaveBeenCalledWith(
        expect.objectContaining({
          Cmd: ['squid', '-k', 'reconfigure'],
        })
      );
    });
  });

  describe('onTeardown', () => {
    it('should remove the ACL file and regenerate master include', async () => {
      await manager.onSpawn('agent-rm', '172.19.0.9', {
        outbound: ['example.com'],
      });
      expect(manager.activeCount).toBe(1);

      vi.mocked(fs.writeFileSync).mockClear();
      vi.mocked(fs.unlinkSync).mockClear();

      await manager.onTeardown('agent-rm');

      // File removed
      const unlinkPath = String(vi.mocked(fs.unlinkSync).mock.calls[0]![0]);
      expect(unlinkPath.replace(/\\/g, '/')).toContain('agent-agent-rm.conf');
      expect(manager.activeCount).toBe(0);

      // Master include regenerated without the removed agent
      const masterCall = findMasterWrite();
      expect(masterCall).toBeDefined();
      expect(String(masterCall![1])).not.toContain('agent-agent-rm.conf');
    });

    it('should be a no-op for unknown instance IDs', async () => {
      await manager.onTeardown('unknown-instance');
      expect(vi.mocked(fs.unlinkSync)).not.toHaveBeenCalled();
    });
  });

  describe('registerCoreAcl', () => {
    it('should write a blanket allow ACL for sera-core', async () => {
      await manager.registerCoreAcl('172.19.0.2');

      const coreCall = findWriteCall('sera-core.conf');
      expect(coreCall).toBeDefined();

      const content = String(coreCall![1]);
      expect(content).toContain('acl sera_core src 172.19.0.2/32');
      expect(content).toContain('http_access allow sera_core');
    });

    it('should include sera-core ACL in master include when it exists', async () => {
      // existsSync returns true for sera-core.conf check
      vi.mocked(fs.existsSync).mockReturnValue(true);

      await manager.registerCoreAcl('172.19.0.2');

      const masterCall = findMasterWrite();
      expect(masterCall).toBeDefined();
      expect(String(masterCall![1])).toContain('include /etc/squid/acls/sera-core.conf');
    });
  });

  describe('multiple agents', () => {
    it('should maintain correct master include with multiple agents', async () => {
      await manager.onSpawn('agent-a', '172.19.0.5', { outbound: ['github.com'] });
      await manager.onSpawn('agent-b', '172.19.0.6', { outbound: ['*'] });

      expect(manager.activeCount).toBe(2);

      const lastMaster = String(findMasterWrite()![1]);
      expect(lastMaster).toContain('agent-agent-a.conf');
      expect(lastMaster).toContain('agent-agent-b.conf');

      // Teardown one
      vi.mocked(fs.writeFileSync).mockClear();
      await manager.onTeardown('agent-a');

      expect(manager.activeCount).toBe(1);
      const afterMaster = String(findMasterWrite()![1]);
      expect(afterMaster).not.toContain('agent-agent-a.conf');
      expect(afterMaster).toContain('agent-agent-b.conf');
    });
  });
});
