import { describe, it, expect, vi, beforeEach } from 'vitest';
import { manageAgentSkill } from './manage-agent.js';
import type { AgentContext } from '../types.js';
import type { SecurityTier } from '../../agents/manifest/types.js';

// Mock database
vi.mock('../../lib/database.js', () => ({
  query: vi.fn(),
}));

// Mock global fetch
const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

import { query } from '../../lib/database.js';
const mockQuery = vi.mocked(query);

const mockContext: AgentContext = {
  agentName: 'sera',
  workspacePath: '/tmp/test',
  tier: 1 as SecurityTier,
  manifest: {
    apiVersion: 'v1',
    kind: 'Agent',
    metadata: {
      name: 'sera',
      displayName: 'Sera',
      icon: '',
      circle: 'default',
      tier: 1 as SecurityTier,
    },
    identity: { role: 'orchestrator', description: 'Main agent' },
    model: { provider: 'openai', name: 'gpt-4' },
  },
  agentInstanceId: 'sera-001',
  containerId: 'container-001',
  sandboxManager: {} as never,
  sessionId: 'session-001',
};

describe('manage-agent skill', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ── list-agents ────────────────────────────────────────────────────────────

  describe('list-agents action', () => {
    it('returns agents list from API', async () => {
      const agents = [
        {
          id: 'agent-1',
          name: 'worker-a',
          display_name: 'Worker A',
          status: 'running',
          template_ref: 'worker',
          lifecycle_mode: 'persistent',
        },
      ];
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => agents,
      });

      const result = await manageAgentSkill.handler({ action: 'list-agents' }, mockContext);

      expect(result.success).toBe(true);
      expect(result.data).toEqual(
        expect.objectContaining({
          agents: expect.arrayContaining([
            expect.objectContaining({
              id: 'agent-1',
              name: 'worker-a',
              status: 'running',
            }),
          ]),
        })
      );
    });

    it('passes statusFilter as query param', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => [],
      });

      await manageAgentSkill.handler(
        { action: 'list-agents', statusFilter: 'running' },
        mockContext
      );

      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringContaining('status=running'),
        expect.any(Object)
      );
    });

    it('returns error on API failure', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        statusText: 'Internal Server Error',
        json: async () => ({ error: 'DB connection failed' }),
      });

      const result = await manageAgentSkill.handler({ action: 'list-agents' }, mockContext);
      expect(result.success).toBe(false);
      expect(result.error).toContain('list-agents failed');
    });
  });

  // ── start-agent ────────────────────────────────────────────────────────────

  describe('start-agent action', () => {
    it('starts agent by name', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [{ id: 'agent-1', name: 'worker-a' }],
        rowCount: 1,
      } as never);
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ id: 'agent-1', name: 'worker-a', status: 'running' }),
      });

      const result = await manageAgentSkill.handler(
        { action: 'start-agent', agentName: 'worker-a' },
        mockContext
      );

      expect(result.success).toBe(true);
      expect(result.data).toEqual(
        expect.objectContaining({
          agentId: 'agent-1',
          agentName: 'worker-a',
          message: expect.stringContaining('started'),
        })
      );
    });

    it('starts agent by id', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [{ id: 'agent-1', name: 'worker-a' }],
        rowCount: 1,
      } as never);
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ id: 'agent-1', status: 'running' }),
      });

      const result = await manageAgentSkill.handler(
        { action: 'start-agent', agentId: 'agent-1' },
        mockContext
      );

      expect(result.success).toBe(true);
      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringContaining('/instances/agent-1/start'),
        expect.any(Object)
      );
    });

    it('returns error when agentName and agentId are missing', async () => {
      const result = await manageAgentSkill.handler({ action: 'start-agent' }, mockContext);
      expect(result.success).toBe(false);
      expect(result.error).toContain('agentName or agentId is required');
    });

    it('returns error when agent not found', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [], rowCount: 0 } as never);

      const result = await manageAgentSkill.handler(
        { action: 'start-agent', agentName: 'nonexistent' },
        mockContext
      );
      expect(result.success).toBe(false);
      expect(result.error).toContain('nonexistent');
    });

    it('returns error on API failure', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [{ id: 'agent-1', name: 'worker-a' }],
        rowCount: 1,
      } as never);
      mockFetch.mockResolvedValueOnce({
        ok: false,
        statusText: 'Conflict',
        json: async () => ({ error: 'Agent already running' }),
      });

      const result = await manageAgentSkill.handler(
        { action: 'start-agent', agentName: 'worker-a' },
        mockContext
      );
      expect(result.success).toBe(false);
      expect(result.error).toContain('start-agent failed');
    });
  });

  // ── stop-agent ─────────────────────────────────────────────────────────────

  describe('stop-agent action', () => {
    it('stops agent by name', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [{ id: 'agent-1', name: 'worker-a' }],
        rowCount: 1,
      } as never);
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ id: 'agent-1', name: 'worker-a', status: 'stopped' }),
      });

      const result = await manageAgentSkill.handler(
        { action: 'stop-agent', agentName: 'worker-a' },
        mockContext
      );

      expect(result.success).toBe(true);
      expect(result.data).toEqual(
        expect.objectContaining({
          agentName: 'worker-a',
          message: expect.stringContaining('stopped'),
        })
      );
      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringContaining('/instances/agent-1/stop'),
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('returns error when agent not found', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [], rowCount: 0 } as never);

      const result = await manageAgentSkill.handler(
        { action: 'stop-agent', agentName: 'ghost' },
        mockContext
      );
      expect(result.success).toBe(false);
      expect(result.error).toContain('ghost');
    });
  });

  // ── restart-agent ──────────────────────────────────────────────────────────

  describe('restart-agent action', () => {
    it('restarts agent by name', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [{ id: 'agent-1', name: 'worker-a' }],
        rowCount: 1,
      } as never);
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ restarted: true, agentName: 'worker-a' }),
      });

      const result = await manageAgentSkill.handler(
        { action: 'restart-agent', agentName: 'worker-a' },
        mockContext
      );

      expect(result.success).toBe(true);
      expect(result.data).toEqual(
        expect.objectContaining({
          agentName: 'worker-a',
          message: expect.stringContaining('restarted'),
        })
      );
      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/agents/agent-1/restart'),
        expect.objectContaining({ method: 'POST' })
      );
    });

    it('returns error on API failure (e.g. non-persistent agent)', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [{ id: 'agent-2', name: 'ephemeral-x' }],
        rowCount: 1,
      } as never);
      mockFetch.mockResolvedValueOnce({
        ok: false,
        statusText: 'Conflict',
        json: async () => ({ error: 'Only persistent agents can be restarted' }),
      });

      const result = await manageAgentSkill.handler(
        { action: 'restart-agent', agentName: 'ephemeral-x' },
        mockContext
      );
      expect(result.success).toBe(false);
      expect(result.error).toContain('restart-agent failed');
    });
  });

  // ── agent-health ───────────────────────────────────────────────────────────

  describe('agent-health action', () => {
    it('returns health status and checks', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [{ id: 'agent-1', name: 'worker-a' }],
        rowCount: 1,
      } as never);
      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          status: 'healthy',
          checks: {
            instance: { ok: true },
            container: { ok: true },
          },
        }),
      });

      const result = await manageAgentSkill.handler(
        { action: 'agent-health', agentName: 'worker-a' },
        mockContext
      );

      expect(result.success).toBe(true);
      expect(result.data).toEqual(
        expect.objectContaining({
          agentId: 'agent-1',
          agentName: 'worker-a',
          status: 'healthy',
          checks: expect.objectContaining({
            instance: expect.objectContaining({ ok: true }),
          }),
        })
      );
      expect(mockFetch).toHaveBeenCalledWith(
        expect.stringContaining('/api/agents/agent-1/health-check'),
        expect.any(Object)
      );
    });

    it('returns error when agent not found in DB', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [], rowCount: 0 } as never);

      const result = await manageAgentSkill.handler(
        { action: 'agent-health', agentName: 'missing-agent' },
        mockContext
      );
      expect(result.success).toBe(false);
      expect(result.error).toContain('missing-agent');
    });
  });

  // ── unknown action ─────────────────────────────────────────────────────────

  describe('unknown action', () => {
    it('returns descriptive error with valid actions list', async () => {
      const result = await manageAgentSkill.handler({ action: 'frobnicate' }, mockContext);
      expect(result.success).toBe(false);
      expect(result.error).toContain('Valid actions:');
      expect(result.error).toContain('start-agent');
      expect(result.error).toContain('list-agents');
    });
  });
});
