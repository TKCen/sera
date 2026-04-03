import { describe, it, expect, vi, beforeEach } from 'vitest';
import { pool } from '../lib/database.js';
import type { QueryResult } from 'pg';

// Mock MCP SDK Server to avoid ajv-formats dependency issue
vi.mock('@modelcontextprotocol/sdk/server/index.js', () => {
  const MockServer = class {
    setRequestHandler = vi.fn();
  };
  return { Server: MockServer };
});

vi.mock('@modelcontextprotocol/sdk/types.js', () => ({
  CallToolRequestSchema: Symbol('CallToolRequestSchema'),
  ListToolsRequestSchema: Symbol('ListToolsRequestSchema'),
}));

// Mock database
vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn().mockResolvedValue({ rows: [], rowCount: 0 }),
  },
  query: vi.fn().mockResolvedValue({ rows: [], rowCount: 0 }),
}));

// Must import after mocks
const { SeraMCPServer } = await import('./SeraMCPServer.js');

type SeraMCPServerInstance = InstanceType<typeof SeraMCPServer>;

// Minimal mock orchestrator
const mockOrchestrator = {
  listAgents: vi.fn().mockReturnValue([]),
  getAgent: vi.fn(),
  getAgentInfo: vi.fn(),
  getIntercom: vi.fn(),
} as unknown as ConstructorParameters<typeof SeraMCPServer>[0];

const mockQueryResult = (rows: unknown[], rowCount?: number): QueryResult => ({
  rows: rows as Record<string, unknown>[],
  rowCount: rowCount ?? rows.length,
  command: 'SELECT',
  oid: 0,
  fields: [],
});

describe('SeraMCPServer — Schedule tools (#647)', () => {
  let server: SeraMCPServerInstance;

  beforeEach(() => {
    vi.clearAllMocks();
    server = new SeraMCPServer(mockOrchestrator);
  });

  // ── schedules.list ──────────────────────────────────────────────────────

  describe('schedules.list', () => {
    it('returns schedules for the given agent', async () => {
      const mockRows = [
        {
          id: 'sched-1',
          name: 'Daily Summary',
          cron: '0 9 * * *',
          task: '{"prompt":"summarize"}',
          status: 'active',
          category: 'general',
          source: 'manifest',
          description: null,
          last_run_at: null,
          next_run_at: '2026-04-04T09:00:00Z',
          last_run_status: null,
          created_at: '2026-04-01T00:00:00Z',
          updated_at: '2026-04-01T00:00:00Z',
        },
      ];
      vi.mocked(pool.query as (...args: unknown[]) => Promise<QueryResult>).mockResolvedValueOnce(
        mockQueryResult(mockRows)
      );

      const result = await server.callTool('schedules.list', { agentId: 'agent-123' });
      expect(result.content[0]!.type).toBe('text');
      const parsed = JSON.parse((result.content[0] as { text: string }).text);
      expect(parsed).toHaveLength(1);
      expect(parsed[0].name).toBe('Daily Summary');
      expect(parsed[0].cron).toBe('0 9 * * *');

      expect(pool.query).toHaveBeenCalledWith(
        expect.stringContaining('FROM schedules WHERE agent_instance_id'),
        ['agent-123']
      );
    });

    it('throws when agentId is missing', async () => {
      await expect(server.callTool('schedules.list', {})).rejects.toThrow('agentId is required');
    });
  });

  // ── schedules.get ───────────────────────────────────────────────────────

  describe('schedules.get', () => {
    it('returns a single schedule', async () => {
      const mockRow = {
        id: 'sched-1',
        name: 'Daily Summary',
        cron: '0 9 * * *',
        status: 'active',
        source: 'api',
      };
      vi.mocked(pool.query as (...args: unknown[]) => Promise<QueryResult>).mockResolvedValueOnce(
        mockQueryResult([mockRow])
      );

      const result = await server.callTool('schedules.get', {
        agentId: 'agent-123',
        scheduleId: 'sched-1',
      });
      const parsed = JSON.parse((result.content[0] as { text: string }).text);
      expect(parsed.name).toBe('Daily Summary');
    });

    it('throws when schedule not found', async () => {
      vi.mocked(pool.query as (...args: unknown[]) => Promise<QueryResult>).mockResolvedValueOnce(
        mockQueryResult([], 0)
      );

      await expect(
        server.callTool('schedules.get', {
          agentId: 'agent-123',
          scheduleId: 'nonexistent',
        })
      ).rejects.toThrow('not found');
    });
  });

  // ── schedules.pause ─────────────────────────────────────────────────────

  describe('schedules.pause', () => {
    it('pauses an active schedule', async () => {
      vi.mocked(pool.query as (...args: unknown[]) => Promise<QueryResult>).mockResolvedValueOnce(
        mockQueryResult([], 1)
      );

      const result = await server.callTool('schedules.pause', {
        agentId: 'agent-123',
        scheduleId: 'sched-1',
      });
      expect((result.content[0] as { text: string }).text).toContain('paused');
      expect(pool.query).toHaveBeenCalledWith(expect.stringContaining("status = 'paused'"), [
        'sched-1',
        'agent-123',
      ]);
    });

    it('returns error when schedule not found or not active', async () => {
      vi.mocked(pool.query as (...args: unknown[]) => Promise<QueryResult>).mockResolvedValueOnce(
        mockQueryResult([], 0)
      );

      await expect(
        server.callTool('schedules.pause', {
          agentId: 'agent-123',
          scheduleId: 'sched-1',
        })
      ).rejects.toThrow('not currently active');
    });
  });

  // ── schedules.resume ────────────────────────────────────────────────────

  describe('schedules.resume', () => {
    it('resumes a paused schedule', async () => {
      vi.mocked(pool.query as (...args: unknown[]) => Promise<QueryResult>).mockResolvedValueOnce(
        mockQueryResult([], 1)
      );

      const result = await server.callTool('schedules.resume', {
        agentId: 'agent-123',
        scheduleId: 'sched-1',
      });
      expect((result.content[0] as { text: string }).text).toContain('resumed');
      expect(pool.query).toHaveBeenCalledWith(expect.stringContaining("status = 'active'"), [
        'sched-1',
        'agent-123',
      ]);
    });

    it('returns error when schedule not found or not paused', async () => {
      vi.mocked(pool.query as (...args: unknown[]) => Promise<QueryResult>).mockResolvedValueOnce(
        mockQueryResult([], 0)
      );

      await expect(
        server.callTool('schedules.resume', {
          agentId: 'agent-123',
          scheduleId: 'sched-1',
        })
      ).rejects.toThrow('not currently paused');
    });
  });

  // ── Tool definitions ────────────────────────────────────────────────────

  describe('tool definitions', () => {
    it('includes all 4 schedule tools', () => {
      const tools = server.getToolDefinitions();
      const scheduleTools = tools.filter((t: { name: string }) => t.name.startsWith('schedules.'));
      expect(scheduleTools).toHaveLength(4);
      expect(scheduleTools.map((t: { name: string }) => t.name).sort()).toEqual([
        'schedules.get',
        'schedules.list',
        'schedules.pause',
        'schedules.resume',
      ]);
    });

    it('schedules.list requires agentId', () => {
      const tools = server.getToolDefinitions();
      const listTool = tools.find((t: { name: string }) => t.name === 'schedules.list') as {
        inputSchema: { required: string[] };
      };
      expect(listTool.inputSchema.required).toContain('agentId');
    });
  });
});
