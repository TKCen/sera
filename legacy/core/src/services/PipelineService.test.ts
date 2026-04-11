import { describe, it, expect, vi, beforeEach } from 'vitest';
import { PipelineService } from './PipelineService.js';
import { query } from '../lib/database.js';

vi.mock('../lib/database.js', () => ({
  query: vi.fn(),
}));

vi.mock('uuid', () => ({
  v4: vi.fn(() => 'test-uuid-1234'),
}));

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

describe('PipelineService', () => {
  let pipelineService: PipelineService;

  beforeEach(() => {
    vi.clearAllMocks();
    pipelineService = PipelineService.getInstance();
  });

  describe('getInstance', () => {
    it('returns a singleton instance', () => {
      const instance1 = PipelineService.getInstance();
      const instance2 = PipelineService.getInstance();
      expect(instance1).toBe(instance2);
    });
  });

  describe('create', () => {
    it('creates a new pipeline and returns it', async () => {
      const mockSteps = [
        { description: 'step 1', status: 'pending' as const },
        { description: 'step 2', status: 'pending' as const },
      ];

      const mockCreatedRow = {
        id: 'test-uuid-1234',
        type: 'sequential',
        status: 'pending',
        steps: mockSteps,
        created_at: new Date('2023-10-10T10:00:00Z'),
      };

      vi.mocked(query)
        .mockResolvedValueOnce({ rows: [] } as unknown as never) // first call (INSERT ignores return rows currently)
        .mockResolvedValueOnce({ rows: [mockCreatedRow] } as unknown as never); // second call is get()

      const result = await pipelineService.create('sequential', mockSteps);

      expect(query).toHaveBeenCalledWith(
        `INSERT INTO pipelines (id, type, status, steps) VALUES ($1, $2, 'pending', $3)`,
        ['test-uuid-1234', 'sequential', JSON.stringify(mockSteps)]
      );

      expect(result).toEqual({
        id: 'test-uuid-1234',
        type: 'sequential',
        status: 'pending',
        steps: mockSteps,
        createdAt: '2023-10-10T10:00:00.000Z',
      });
    });
  });

  describe('get', () => {
    it('returns null if pipeline is not found', async () => {
      vi.mocked(query).mockResolvedValue({ rows: [] } as unknown as never);

      const result = await pipelineService.get('nonexistent');

      expect(result).toBeNull();
      expect(query).toHaveBeenCalledWith(`SELECT * FROM pipelines WHERE id = $1`, ['nonexistent']);
    });

    it('returns formatted pipeline if found', async () => {
      const mockRow = {
        id: '123',
        type: 'parallel',
        status: 'completed',
        steps: [],
        created_at: new Date('2023-10-10T10:00:00Z'),
        completed_at: new Date('2023-10-10T10:05:00Z'),
      };
      vi.mocked(query).mockResolvedValue({ rows: [mockRow] } as unknown as never);

      const result = await pipelineService.get('123');

      expect(result).toEqual({
        id: '123',
        type: 'parallel',
        status: 'completed',
        steps: [],
        createdAt: '2023-10-10T10:00:00.000Z',
        completedAt: '2023-10-10T10:05:00.000Z',
      });
    });
  });

  describe('updateStatus', () => {
    it('updates status and completed_at when status is completed', async () => {
      await pipelineService.updateStatus('123', 'completed');

      expect(query).toHaveBeenCalledWith(
        `UPDATE pipelines SET status = $1, completed_at = NOW() WHERE id = $2`,
        ['completed', '123']
      );
    });

    it('updates status and completed_at when status is failed', async () => {
      await pipelineService.updateStatus('123', 'failed');

      expect(query).toHaveBeenCalledWith(
        `UPDATE pipelines SET status = $1, completed_at = NOW() WHERE id = $2`,
        ['failed', '123']
      );
    });

    it('only updates status for other states like running', async () => {
      await pipelineService.updateStatus('123', 'running');

      expect(query).toHaveBeenCalledWith(`UPDATE pipelines SET status = $1 WHERE id = $2`, [
        'running',
        '123',
      ]);
    });
  });

  describe('updateSteps', () => {
    it('updates steps json', async () => {
      const newSteps = [{ description: 'new step', status: 'completed' as const }];

      await pipelineService.updateSteps('123', newSteps);

      expect(query).toHaveBeenCalledWith(`UPDATE pipelines SET steps = $1 WHERE id = $2`, [
        JSON.stringify(newSteps),
        '123',
      ]);
    });
  });
});
