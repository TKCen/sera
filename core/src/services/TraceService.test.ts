import { describe, it, expect, vi, beforeEach } from 'vitest';
import { TraceService, TraceAccumulator } from './TraceService.js';
import { pool } from '../lib/database.js';
import type { Mock } from 'vitest';

// ── Mocks ─────────────────────────────────────────────────────────────────────

vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn(),
  },
}));

vi.mock('../lib/logger.js', () => {
  const Logger = vi.fn();
  Logger.prototype.info = vi.fn();
  Logger.prototype.debug = vi.fn();
  Logger.prototype.warn = vi.fn();
  Logger.prototype.error = vi.fn();
  return { Logger };
});

const mockQuery = pool.query as unknown as Mock;

// ── Helpers ───────────────────────────────────────────────────────────────────

function resetSingletons() {
  // Reset accumulator internal state between tests
  TraceAccumulator.getInstance().clear();
  // Reset TraceService singleton so it re-fetches the accumulator instance
  (TraceService as unknown as { instance: undefined }).instance = undefined;
}

// ── Tests ─────────────────────────────────────────────────────────────────────

describe('TraceAccumulator', () => {
  beforeEach(() => {
    resetSingletons();
  });

  it('creates a new trace data entry on first access', () => {
    const acc = TraceAccumulator.getInstance();
    const data = acc.getOrCreate('agent-1', 'session-1');
    expect(data.messages).toEqual([]);
    expect(data.toolUses).toEqual([]);
    expect(data.totalTokens).toBe(0);
    expect(data.startedAt).toBeTruthy();
  });

  it('returns the same entry on subsequent access', () => {
    const acc = TraceAccumulator.getInstance();
    const data1 = acc.getOrCreate('agent-1', 'session-1');
    data1.messages.push({ role: 'user', content: 'hello' });
    const data2 = acc.getOrCreate('agent-1', 'session-1');
    expect(data2.messages).toHaveLength(1);
  });

  it('keys are scoped to agentInstanceId::sessionId', () => {
    const acc = TraceAccumulator.getInstance();
    acc.addMessage('agent-1', 'session-A', { role: 'user', content: 'msg A' });
    acc.addMessage('agent-1', 'session-B', { role: 'user', content: 'msg B' });
    expect(acc.get('agent-1', 'session-A')!.messages).toHaveLength(1);
    expect(acc.get('agent-1', 'session-B')!.messages).toHaveLength(1);
    expect(acc.get('agent-1', 'session-A')!.messages[0]!.content).toBe('msg A');
  });

  it('accumulates messages', () => {
    const acc = TraceAccumulator.getInstance();
    acc.addMessage('agent-1', 'session-1', { role: 'user', content: 'hello' });
    acc.addMessage('agent-1', 'session-1', { role: 'assistant', content: 'hi there' });
    const data = acc.get('agent-1', 'session-1')!;
    expect(data.messages).toHaveLength(2);
    expect(data.messages[1]!.role).toBe('assistant');
  });

  it('accumulates tool uses', () => {
    const acc = TraceAccumulator.getInstance();
    acc.addToolUse('agent-1', 'session-1', {
      toolName: 'read_file',
      arguments: { path: '/tmp/foo' },
      result: 'file contents',
      durationMs: 42,
    });
    const data = acc.get('agent-1', 'session-1')!;
    expect(data.toolUses).toHaveLength(1);
    expect(data.toolUses[0]!.toolName).toBe('read_file');
  });

  it('accumulates token counts', () => {
    const acc = TraceAccumulator.getInstance();
    acc.recordTokens('agent-1', 'session-1', 100, 50);
    acc.recordTokens('agent-1', 'session-1', 200, 80);
    const data = acc.get('agent-1', 'session-1')!;
    expect(data.promptTokens).toBe(300);
    expect(data.completionTokens).toBe(130);
    expect(data.totalTokens).toBe(430);
  });

  it('sets model', () => {
    const acc = TraceAccumulator.getInstance();
    acc.setModel('agent-1', 'session-1', 'gpt-4o');
    expect(acc.get('agent-1', 'session-1')!.model).toBe('gpt-4o');
  });

  it('finalize removes the entry and sets completedAt', () => {
    const acc = TraceAccumulator.getInstance();
    acc.addMessage('agent-1', 'session-1', { role: 'user', content: 'test' });
    const data = acc.finalize('agent-1', 'session-1');
    expect(data).not.toBeNull();
    expect(data!.completedAt).toBeTruthy();
    expect(acc.get('agent-1', 'session-1')).toBeUndefined();
  });

  it('finalize returns undefined for unknown key', () => {
    const acc = TraceAccumulator.getInstance();
    const result = acc.finalize('missing', 'session');
    expect(result).toBeUndefined();
  });
});

describe('TraceService', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetSingletons();
  });

  describe('persist', () => {
    it('persists accumulated trace data to the database', async () => {
      const service = TraceService.getInstance();
      service.addMessage('agent-1', 'sess-1', { role: 'user', content: 'hello world' });
      service.addMessage('agent-1', 'sess-1', { role: 'assistant', content: 'hi there' });
      service.recordTokens('agent-1', 'sess-1', 50, 25);
      service.setModel('agent-1', 'sess-1', 'test-model');

      const mockTrace = {
        id: 'trace-uuid-1',
        agent_instance_id: 'agent-1',
        session_id: 'sess-1',
        trace_data: {},
        summary:
          'User: hello world\n\nAssistant: hi there\n\nTokens: 50 prompt + 25 completion = 75 total',
        token_count: 75,
        created_at: new Date(),
        updated_at: new Date(),
      };
      mockQuery.mockResolvedValueOnce({ rows: [mockTrace], rowCount: 1 });

      const result = await service.persist('agent-1', 'sess-1');
      expect(result).not.toBeNull();
      expect(result!.id).toBe('trace-uuid-1');
      expect(mockQuery).toHaveBeenCalledOnce();

      const call = mockQuery.mock.calls[0]!;
      expect(call[0]).toContain('INSERT INTO interaction_traces');
      const params = call[1] as unknown[];
      expect(params[0]).toBe('agent-1');
      expect(params[1]).toBe('sess-1');
    });

    it('returns null when no accumulator entry exists', async () => {
      const service = TraceService.getInstance();
      const result = await service.persist('missing-agent', 'missing-session');
      expect(result).toBeNull();
      expect(mockQuery).not.toHaveBeenCalled();
    });

    it('returns null and logs on database error', async () => {
      const service = TraceService.getInstance();
      service.addMessage('agent-1', 'sess-err', { role: 'user', content: 'test' });
      mockQuery.mockRejectedValueOnce(new Error('DB connection failed'));

      const result = await service.persist('agent-1', 'sess-err');
      expect(result).toBeNull();
    });
  });

  describe('listTraces', () => {
    it('queries all traces without agentId', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [], rowCount: 0 });
      const result = await TraceService.getInstance().listTraces();
      expect(result).toEqual([]);
      const call = mockQuery.mock.calls[0]!;
      expect(call[0] as string).not.toContain('WHERE');
    });

    it('queries traces filtered by agentId', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [], rowCount: 0 });
      await TraceService.getInstance().listTraces('agent-xyz');
      const call = mockQuery.mock.calls[0]!;
      expect(call[0] as string).toContain('WHERE agent_instance_id');
      expect((call[1] as unknown[])[0]).toBe('agent-xyz');
    });

    it('passes the provided limit to the query', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [], rowCount: 0 });
      await TraceService.getInstance().listTraces(undefined, 100);
      const call = mockQuery.mock.calls[0]!;
      expect((call[1] as unknown[])[0]).toBe(100);
    });
  });

  describe('getTrace', () => {
    it('returns the trace when found', async () => {
      const mockTrace = { id: 'trace-1', agent_instance_id: 'agent-1' };
      mockQuery.mockResolvedValueOnce({ rows: [mockTrace], rowCount: 1 });
      const result = await TraceService.getInstance().getTrace('trace-1');
      expect(result).toEqual(mockTrace);
    });

    it('returns null when not found', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [], rowCount: 0 });
      const result = await TraceService.getInstance().getTrace('missing');
      expect(result).toBeNull();
    });
  });

  describe('getTracesBySession', () => {
    it('queries by agent and session', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [], rowCount: 0 });
      await TraceService.getInstance().getTracesBySession('agent-1', 'session-abc');
      const call = mockQuery.mock.calls[0]!;
      expect(call[0] as string).toContain('WHERE agent_instance_id');
      const params = call[1] as unknown[];
      expect(params[0]).toBe('agent-1');
      expect(params[1]).toBe('session-abc');
    });
  });

  describe('deleteTrace', () => {
    it('returns true when a row was deleted', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [], rowCount: 1 });
      const result = await TraceService.getInstance().deleteTrace('trace-1');
      expect(result).toBe(true);
    });

    it('returns false when no row was deleted', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [], rowCount: 0 });
      const result = await TraceService.getInstance().deleteTrace('missing');
      expect(result).toBe(false);
    });
  });
});
