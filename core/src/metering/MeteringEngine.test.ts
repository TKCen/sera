import { describe, it, expect, vi, beforeEach } from 'vitest';
import { MeteringEngine, type UsageEvent } from './MeteringEngine.js';
import { query } from '../lib/database.js';

// Mock database query
vi.mock('../lib/database.js', () => ({
  query: vi.fn(),
}));

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

describe('MeteringEngine', () => {
  let engine: MeteringEngine;

  beforeEach(() => {
    engine = new MeteringEngine();
    vi.clearAllMocks();
  });

  describe('record', () => {
    it('should insert usage event into database', async () => {
      const event: UsageEvent = {
        agentId: 'agent-123',
        model: 'gpt-4',
        promptTokens: 100,
        completionTokens: 50,
        totalTokens: 150,
      };

      vi.mocked(query).mockResolvedValueOnce({ rows: [], rowCount: 1, command: 'INSERT', oid: 0, fields: [] });

      await engine.record(event);

      expect(query).toHaveBeenCalledWith(
        expect.stringContaining('INSERT INTO usage_events'),
        [event.agentId, event.model, event.promptTokens, event.completionTokens, event.totalTokens]
      );
    });

    it('should log error if database query fails', async () => {
      const event: UsageEvent = {
        agentId: 'agent-123',
        model: 'gpt-4',
        promptTokens: 100,
        completionTokens: 50,
        totalTokens: 150,
      };

      vi.mocked(query).mockRejectedValueOnce(new Error('Database error'));

      // Should not throw, but handle error internally
      await expect(engine.record(event)).resolves.not.toThrow();
      expect(query).toHaveBeenCalled();
    });
  });
});
