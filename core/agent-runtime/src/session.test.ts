import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import fs from 'fs';
import path from 'path';
import { SessionStore } from './session.js';
import type { SerializedSession } from './session.js';
import type { ChatMessage } from './llmClient.js';

vi.mock('fs');
vi.mock('./logger.js', () => ({
  log: vi.fn(),
}));

describe('SessionStore', () => {
  const workspacePath = '/workspace';
  const sessionPath = path.join(workspacePath, '.sera', 'session.json');
  let store: SessionStore;

  beforeEach(() => {
    vi.resetAllMocks();
    vi.useFakeTimers();
    store = new SessionStore(workspacePath);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe('serialize', () => {
    it('serializes messages into portable format', () => {
      const messages: ChatMessage[] = [
        { role: 'system', content: 'You are helpful.' },
        { role: 'user', content: 'Hello' },
        { role: 'assistant', content: 'Hi there!' },
      ];

      const session = store.serialize(messages, 'agent-1', 'task-1', {
        promptTokens: 100,
        completionTokens: 50,
      });

      expect(session.version).toBe(1);
      expect(session.agentId).toBe('agent-1');
      expect(session.taskId).toBe('task-1');
      expect(session.messages).toHaveLength(3);
      expect(session.totalUsage).toEqual({ promptTokens: 100, completionTokens: 50 });
      expect(session.createdAt).toBeDefined();
      expect(session.updatedAt).toBeDefined();
    });

    it('filters out internal messages', () => {
      const messages: ChatMessage[] = [
        { role: 'system', content: 'You are helpful.' },
        { role: 'system', content: 'Internal context', internal: true },
        { role: 'user', content: 'Hello' },
      ];

      const session = store.serialize(messages, 'agent-1', 'task-1', {
        promptTokens: 0,
        completionTokens: 0,
      });

      expect(session.messages).toHaveLength(2);
      expect(session.messages[0]!.content).toBe('You are helpful.');
      expect(session.messages[1]!.content).toBe('Hello');
    });

    it('preserves tool_calls and tool_call_id', () => {
      const toolCalls = [
        {
          id: 'call_1',
          type: 'function' as const,
          function: { name: 'echo', arguments: '{"text":"hi"}' },
        },
      ];
      const messages: ChatMessage[] = [
        { role: 'assistant', content: 'Using tool', tool_calls: toolCalls },
        { role: 'tool', content: 'hi', tool_call_id: 'call_1' },
      ];

      const session = store.serialize(messages, 'agent-1', 'task-1', {
        promptTokens: 0,
        completionTokens: 0,
      });

      expect(session.messages[0]!.tool_calls).toEqual(toolCalls);
      expect(session.messages[1]!.tool_call_id).toBe('call_1');
    });

    it('converts array content blocks to string', () => {
      const messages: ChatMessage[] = [
        {
          role: 'user',
          content: [
            { type: 'text', text: 'Hello ' },
            { type: 'text', text: 'world' },
          ],
        },
      ];

      const session = store.serialize(messages, 'agent-1', 'task-1', {
        promptTokens: 0,
        completionTokens: 0,
      });

      expect(session.messages[0]!.content).toBe('Hello world');
    });
  });

  describe('deserialize', () => {
    it('converts SerializedSession back to ChatMessage[]', () => {
      const session: SerializedSession = {
        version: 1,
        agentId: 'agent-1',
        taskId: 'task-1',
        messages: [
          { role: 'system', content: 'You are helpful.' },
          { role: 'user', content: 'Hello' },
          {
            role: 'assistant',
            content: 'Hi!',
            tool_calls: [
              { id: 'call_1', type: 'function', function: { name: 'echo', arguments: '{}' } },
            ],
          },
          { role: 'tool', content: 'result', tool_call_id: 'call_1' },
        ],
        totalUsage: { promptTokens: 100, completionTokens: 50 },
        createdAt: '2025-01-01T00:00:00.000Z',
        updatedAt: '2025-01-01T00:01:00.000Z',
      };

      const messages = store.deserialize(session);

      expect(messages).toHaveLength(4);
      expect(messages[0]!.role).toBe('system');
      expect(messages[2]!.tool_calls).toHaveLength(1);
      expect(messages[3]!.tool_call_id).toBe('call_1');
    });
  });

  describe('saveSync', () => {
    it('creates directory and writes file', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      vi.mocked(fs.mkdirSync).mockReturnValue(undefined);
      vi.mocked(fs.writeFileSync).mockReturnValue(undefined);

      const session: SerializedSession = {
        version: 1,
        agentId: 'agent-1',
        taskId: 'task-1',
        messages: [],
        totalUsage: { promptTokens: 0, completionTokens: 0 },
        createdAt: '2025-01-01T00:00:00.000Z',
        updatedAt: '2025-01-01T00:00:00.000Z',
      };

      store.saveSync(session);

      expect(fs.mkdirSync).toHaveBeenCalledWith(path.join(workspacePath, '.sera'), {
        recursive: true,
      });
      expect(fs.writeFileSync).toHaveBeenCalledWith(
        sessionPath,
        JSON.stringify(session, null, 2),
        'utf-8'
      );
    });

    it('handles write errors gracefully', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.writeFileSync).mockImplementation(() => {
        throw new Error('Permission denied');
      });

      const session: SerializedSession = {
        version: 1,
        agentId: 'agent-1',
        taskId: 'task-1',
        messages: [],
        totalUsage: { promptTokens: 0, completionTokens: 0 },
        createdAt: '2025-01-01T00:00:00.000Z',
        updatedAt: '2025-01-01T00:00:00.000Z',
      };

      // Should not throw
      expect(() => store.saveSync(session)).not.toThrow();
    });
  });

  describe('save (debounced)', () => {
    it('saves immediately on first call', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.writeFileSync).mockReturnValue(undefined);

      const session: SerializedSession = {
        version: 1,
        agentId: 'agent-1',
        taskId: 'task-1',
        messages: [],
        totalUsage: { promptTokens: 0, completionTokens: 0 },
        createdAt: '2025-01-01T00:00:00.000Z',
        updatedAt: '2025-01-01T00:00:00.000Z',
      };

      store.save(session);

      expect(fs.writeFileSync).toHaveBeenCalledTimes(1);
    });

    it('debounces rapid successive saves', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.writeFileSync).mockReturnValue(undefined);

      const session: SerializedSession = {
        version: 1,
        agentId: 'agent-1',
        taskId: 'task-1',
        messages: [],
        totalUsage: { promptTokens: 0, completionTokens: 0 },
        createdAt: '2025-01-01T00:00:00.000Z',
        updatedAt: '2025-01-01T00:00:00.000Z',
      };

      // First call saves immediately
      store.save(session);
      expect(fs.writeFileSync).toHaveBeenCalledTimes(1);

      // Second call within debounce window is deferred
      store.save(session);
      expect(fs.writeFileSync).toHaveBeenCalledTimes(1);

      // After debounce period, it saves
      vi.advanceTimersByTime(5000);
      expect(fs.writeFileSync).toHaveBeenCalledTimes(2);
    });
  });

  describe('load', () => {
    it('returns null when no session file exists', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);

      const result = store.load();

      expect(result).toBeNull();
    });

    it('loads valid session from disk', () => {
      const session: SerializedSession = {
        version: 1,
        agentId: 'agent-1',
        taskId: 'task-1',
        messages: [{ role: 'user', content: 'Hello' }],
        totalUsage: { promptTokens: 100, completionTokens: 50 },
        createdAt: '2025-01-01T00:00:00.000Z',
        updatedAt: '2025-01-01T00:01:00.000Z',
      };

      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(session));

      const result = store.load();

      expect(result).toEqual(session);
    });

    it('returns null for wrong version', () => {
      const session = {
        version: 99,
        agentId: 'agent-1',
        taskId: 'task-1',
        messages: [],
        totalUsage: { promptTokens: 0, completionTokens: 0 },
        createdAt: '2025-01-01T00:00:00.000Z',
        updatedAt: '2025-01-01T00:00:00.000Z',
      };

      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify(session));

      const result = store.load();

      expect(result).toBeNull();
    });

    it('returns null for invalid JSON', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue('not json');

      const result = store.load();

      expect(result).toBeNull();
    });
  });

  describe('delete', () => {
    it('deletes existing session file', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.unlinkSync).mockReturnValue(undefined);

      store.delete();

      expect(fs.unlinkSync).toHaveBeenCalledWith(sessionPath);
    });

    it('does nothing when no session file exists', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);

      store.delete();

      expect(fs.unlinkSync).not.toHaveBeenCalled();
    });
  });

  describe('cleanup', () => {
    it('removes sessions older than 24 hours', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.statSync).mockReturnValue({
        mtimeMs: Date.now() - 25 * 60 * 60 * 1000, // 25 hours ago
      } as fs.Stats);
      vi.mocked(fs.unlinkSync).mockReturnValue(undefined);

      store.cleanup();

      expect(fs.unlinkSync).toHaveBeenCalledWith(sessionPath);
    });

    it('keeps sessions younger than 24 hours', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.statSync).mockReturnValue({
        mtimeMs: Date.now() - 12 * 60 * 60 * 1000, // 12 hours ago
      } as fs.Stats);

      store.cleanup();

      expect(fs.unlinkSync).not.toHaveBeenCalled();
    });

    it('does nothing when no session file exists', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);

      store.cleanup();

      expect(fs.statSync).not.toHaveBeenCalled();
    });
  });
});
