/**
 * SessionStore unit tests.
 *
 * These test the SessionStore logic by mocking the pg query function.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock the database module
vi.mock('../lib/database.js', () => ({
  query: vi.fn(),
}));

// Mock fs/promises
vi.mock('fs/promises', () => ({
  default: {
    mkdir: vi.fn().mockResolvedValue(undefined),
    writeFile: vi.fn().mockResolvedValue(undefined),
    unlink: vi.fn().mockResolvedValue(undefined),
  },
}));

import { SessionStore } from './SessionStore.js';
import { query } from '../lib/database.js';

const mockQuery = vi.mocked(query);

describe('SessionStore', () => {
  let store: SessionStore;

  beforeEach(() => {
    vi.clearAllMocks();
    store = new SessionStore('/tmp/test-memory');
  });

  describe('createSession', () => {
    it('creates a session with default title', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [],
        rowCount: 1,
      } as unknown as import('pg').QueryResult<Record<string, unknown>>);

      const session = await store.createSession({ agentName: 'architect-prime' });

      expect(session.agentName).toBe('architect-prime');
      expect(session.title).toBe('New Chat');
      expect(session.messageCount).toBe(0);
      expect(session.id).toBeTruthy();
      expect(session.createdAt).toBeTruthy();

      // Verify SQL was called
      expect(mockQuery).toHaveBeenCalledOnce();
      const [sql, params] = mockQuery.mock.calls[0]!;
      expect(sql).toContain('INSERT INTO chat_sessions');
      expect(params![1]).toBe('architect-prime');
    });

    it('creates a session with custom title', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [],
        rowCount: 1,
      } as unknown as import('pg').QueryResult<Record<string, unknown>>);

      const session = await store.createSession({
        agentName: 'researcher',
        title: 'My Research Session',
      });

      expect(session.title).toBe('My Research Session');
    });
  });

  describe('getSession', () => {
    it('returns session when found', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [
          {
            id: 'sess-1',
            agent_name: 'architect-prime',
            title: 'Test Session',
            message_count: 5,
            created_at: '2026-03-17T10:00:00Z',
            updated_at: '2026-03-17T10:05:00Z',
          },
        ],
      } as unknown as import('pg').QueryResult<Record<string, unknown>>);

      const session = await store.getSession('sess-1');

      expect(session).not.toBeNull();
      expect(session!.id).toBe('sess-1');
      expect(session!.agentName).toBe('architect-prime');
      expect(session!.messageCount).toBe(5);
    });

    it('returns null when not found', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [] } as unknown as import('pg').QueryResult<
        Record<string, unknown>
      >);

      const session = await store.getSession('nonexistent');
      expect(session).toBeNull();
    });
  });

  describe('listSessions', () => {
    it('lists all sessions without filter', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [
          {
            id: 's1',
            agent_name: 'a',
            title: 'T1',
            message_count: 2,
            created_at: '2026-03-17T10:00:00Z',
            updated_at: '2026-03-17T10:05:00Z',
          },
          {
            id: 's2',
            agent_name: 'b',
            title: 'T2',
            message_count: 0,
            created_at: '2026-03-17T09:00:00Z',
            updated_at: '2026-03-17T09:00:00Z',
          },
        ],
      } as unknown as import('pg').QueryResult<Record<string, unknown>>);

      const sessions = await store.listSessions();
      expect(sessions).toHaveLength(2);

      const [sql] = mockQuery.mock.calls[0]!;
      expect(sql).not.toContain('WHERE agent_name');
    });

    it('lists sessions filtered by agent', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [] } as unknown as import('pg').QueryResult<
        Record<string, unknown>
      >);

      await store.listSessions('architect-prime');

      const [sql, params] = mockQuery.mock.calls[0]!;
      expect(sql).toContain('WHERE agent_name = $1');
      expect(params![0]).toBe('architect-prime');
    });
  });

  describe('addMessage', () => {
    it('inserts a message and updates session', async () => {
      // First call: INSERT message, Second call: UPDATE session
      mockQuery
        .mockResolvedValueOnce({
          rows: [],
          rowCount: 1,
        } as unknown as import('pg').QueryResult<Record<string, unknown>>)
        .mockResolvedValueOnce({
          rows: [],
          rowCount: 1,
        } as unknown as import('pg').QueryResult<Record<string, unknown>>);

      const msg = await store.addMessage({
        sessionId: 'sess-1',
        role: 'user',
        content: 'Hello!',
      });

      expect(msg.sessionId).toBe('sess-1');
      expect(msg.role).toBe('user');
      expect(msg.content).toBe('Hello!');
      expect(msg.id).toBeTruthy();
      expect(mockQuery).toHaveBeenCalledTimes(2);
    });
  });

  describe('getMessages', () => {
    it('returns messages in order', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [
          {
            id: 'm1',
            session_id: 's1',
            role: 'user',
            content: 'Hi',
            metadata: null,
            created_at: '2026-03-17T10:00:00Z',
          },
          {
            id: 'm2',
            session_id: 's1',
            role: 'assistant',
            content: 'Hello!',
            metadata: null,
            created_at: '2026-03-17T10:00:01Z',
          },
        ],
      } as unknown as import('pg').QueryResult<Record<string, unknown>>);

      const messages = await store.getMessages('s1');
      expect(messages).toHaveLength(2);
      expect(messages[0]!.role).toBe('user');
      expect(messages[1]!.role).toBe('assistant');
    });
  });

  describe('deleteSession', () => {
    it('returns true when session deleted', async () => {
      // getSession query
      mockQuery.mockResolvedValueOnce({
        rows: [
          {
            id: 's1',
            agent_name: 'a',
            title: 'T',
            message_count: 0,
            created_at: '2026-03-17T10:00:00Z',
            updated_at: '2026-03-17T10:00:00Z',
          },
        ],
      } as unknown as import('pg').QueryResult<Record<string, unknown>>);
      // DELETE query
      mockQuery.mockResolvedValueOnce({
        rowCount: 1,
      } as unknown as import('pg').QueryResult<Record<string, unknown>>);

      const result = await store.deleteSession('s1');
      expect(result).toBe(true);
    });

    it('returns false when session not found', async () => {
      // getSession returns nothing
      mockQuery.mockResolvedValueOnce({ rows: [] } as unknown as import('pg').QueryResult<
        Record<string, unknown>
      >);
      // DELETE query
      mockQuery.mockResolvedValueOnce({
        rowCount: 0,
      } as unknown as import('pg').QueryResult<Record<string, unknown>>);

      const result = await store.deleteSession('nonexistent');
      expect(result).toBe(false);
    });
  });

  describe('searchMessages', () => {
    const messageRow = {
      id: 'm1',
      session_id: 's1',
      role: 'user',
      content: 'What is the capital of France?',
      metadata: null,
      created_at: '2026-03-17T10:00:00Z',
    };

    it('queries with agent_instance_id and ILIKE pattern', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [messageRow],
      } as unknown as import('pg').QueryResult<Record<string, unknown>>);

      const results = await store.searchMessages({
        agentInstanceId: 'agent-abc',
        query: 'France',
      });

      expect(results).toHaveLength(1);
      expect(results[0]!.content).toBe('What is the capital of France?');

      const [sql, params] = mockQuery.mock.calls[0]!;
      expect(sql).toContain('ILIKE');
      expect(params![0]).toBe('agent-abc');
      expect(params![1]).toBe('%France%');
    });

    it('applies roles filter', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [] } as unknown as import('pg').QueryResult<
        Record<string, unknown>
      >);

      await store.searchMessages({
        agentInstanceId: 'agent-abc',
        query: 'test',
        roles: ['user', 'assistant'],
      });

      const [sql, params] = mockQuery.mock.calls[0]!;
      expect(sql).toContain('ANY');
      expect(params).toContainEqual(['user', 'assistant']);
    });

    it('applies start_date filter', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [] } as unknown as import('pg').QueryResult<
        Record<string, unknown>
      >);

      await store.searchMessages({
        agentInstanceId: 'agent-abc',
        query: 'test',
        startDate: '2026-01-01T00:00:00Z',
      });

      const [sql, params] = mockQuery.mock.calls[0]!;
      expect(sql).toContain('>=');
      expect(params).toContain('2026-01-01T00:00:00Z');
    });

    it('applies end_date filter', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [] } as unknown as import('pg').QueryResult<
        Record<string, unknown>
      >);

      await store.searchMessages({
        agentInstanceId: 'agent-abc',
        query: 'test',
        endDate: '2026-12-31T23:59:59Z',
      });

      const [sql, params] = mockQuery.mock.calls[0]!;
      expect(sql).toContain('<=');
      expect(params).toContain('2026-12-31T23:59:59Z');
    });

    it('clamps limit to max 50', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [] } as unknown as import('pg').QueryResult<
        Record<string, unknown>
      >);

      await store.searchMessages({
        agentInstanceId: 'agent-abc',
        query: 'test',
        limit: 999,
      });

      const [, params] = mockQuery.mock.calls[0]!;
      expect(params).toContain(50);
    });

    it('defaults limit to 10', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [] } as unknown as import('pg').QueryResult<
        Record<string, unknown>
      >);

      await store.searchMessages({
        agentInstanceId: 'agent-abc',
        query: 'test',
      });

      const [, params] = mockQuery.mock.calls[0]!;
      expect(params).toContain(10);
    });

    it('returns empty array when no matches', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [] } as unknown as import('pg').QueryResult<
        Record<string, unknown>
      >);

      const results = await store.searchMessages({
        agentInstanceId: 'agent-abc',
        query: 'nonexistent',
      });

      expect(results).toHaveLength(0);
    });
  });

  describe('updateSessionTitle', () => {
    it('updates and returns the session', async () => {
      mockQuery.mockResolvedValueOnce({
        rows: [
          {
            id: 's1',
            agent_name: 'a',
            title: 'New Title',
            message_count: 3,
            created_at: '2026-03-17T10:00:00Z',
            updated_at: '2026-03-17T10:05:00Z',
          },
        ],
      } as unknown as import('pg').QueryResult<Record<string, unknown>>);

      const session = await store.updateSessionTitle('s1', 'New Title');
      expect(session).not.toBeNull();
      expect(session!.title).toBe('New Title');
    });

    it('returns null for nonexistent session', async () => {
      mockQuery.mockResolvedValueOnce({ rows: [] } as unknown as import('pg').QueryResult<
        Record<string, unknown>
      >);

      const session = await store.updateSessionTitle('nonexistent', 'Title');
      expect(session).toBeNull();
    });
  });
});
