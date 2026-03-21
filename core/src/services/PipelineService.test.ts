import { describe, it, expect, vi, beforeEach } from 'vitest';
import { PipelineService } from './PipelineService.js';

const queryMock = vi.fn();
vi.mock('../lib/database.js', () => ({
  query: (...args: any[]) => queryMock(...args),
}));

describe('PipelineService', () => {
  let pipelineService: PipelineService;

  beforeEach(() => {
    vi.clearAllMocks();
    // Using getInstance since it's a singleton
    pipelineService = PipelineService.getInstance();
  });

  describe('create', () => {
    it('creates a new pipeline with given type and steps', async () => {
      const mockDate = new Date('2023-10-01T00:00:00Z');
      const steps = [
        { description: 'step 1', status: 'pending' as const },
        { description: 'step 2', status: 'pending' as const },
      ];

      queryMock.mockResolvedValueOnce({ rows: [] }); // For INSERT
      queryMock.mockResolvedValueOnce({ // For SELECT (get)
        rows: [{
          id: 'test-uuid-1',
          type: 'sequential',
          status: 'pending',
          steps: steps,
          created_at: mockDate,
        }],
      });

      const pipeline = await pipelineService.create('sequential', steps);

      expect(queryMock).toHaveBeenCalledTimes(2);
      expect(queryMock).toHaveBeenNthCalledWith(
        1,
        expect.stringContaining('INSERT INTO pipelines'),
        expect.arrayContaining([expect.any(String), 'sequential', expect.any(String)])
      );

      expect(pipeline).toEqual({
        id: 'test-uuid-1',
        type: 'sequential',
        status: 'pending',
        steps: steps,
        createdAt: mockDate.toISOString(),
      });
    });
  });

  describe('get', () => {
    it('returns null if pipeline does not exist', async () => {
      queryMock.mockResolvedValueOnce({ rows: [] });

      const pipeline = await pipelineService.get('non-existent-id');

      expect(pipeline).toBeNull();
      expect(queryMock).toHaveBeenCalledTimes(1);
    });

    it('returns pipeline data if it exists', async () => {
      const mockDate = new Date('2023-10-01T00:00:00Z');
      const mockCompletedDate = new Date('2023-10-01T01:00:00Z');

      queryMock.mockResolvedValueOnce({
        rows: [{
          id: 'existing-id',
          type: 'parallel',
          status: 'completed',
          steps: [],
          created_at: mockDate,
          completed_at: mockCompletedDate,
        }],
      });

      const pipeline = await pipelineService.get('existing-id');

      expect(pipeline).toEqual({
        id: 'existing-id',
        type: 'parallel',
        status: 'completed',
        steps: [],
        createdAt: mockDate.toISOString(),
        completedAt: mockCompletedDate.toISOString(),
      });
    });
  });

  describe('updateStatus', () => {
    it('updates status and completed_at when status is completed', async () => {
      queryMock.mockResolvedValueOnce({ rows: [] });

      await pipelineService.updateStatus('test-id', 'completed');

      expect(queryMock).toHaveBeenCalledTimes(1);
      expect(queryMock).toHaveBeenCalledWith(
        expect.stringContaining('UPDATE pipelines SET status = $1, completed_at = NOW()'),
        ['completed', 'test-id']
      );
    });

    it('updates status and completed_at when status is failed', async () => {
      queryMock.mockResolvedValueOnce({ rows: [] });

      await pipelineService.updateStatus('test-id', 'failed');

      expect(queryMock).toHaveBeenCalledTimes(1);
      expect(queryMock).toHaveBeenCalledWith(
        expect.stringContaining('UPDATE pipelines SET status = $1, completed_at = NOW()'),
        ['failed', 'test-id']
      );
    });

    it('updates only status when status is running', async () => {
      queryMock.mockResolvedValueOnce({ rows: [] });

      await pipelineService.updateStatus('test-id', 'running');

      expect(queryMock).toHaveBeenCalledTimes(1);
      expect(queryMock).toHaveBeenCalledWith(
        expect.stringContaining('UPDATE pipelines SET status = $1 WHERE id = $2'),
        ['running', 'test-id']
      );
    });
  });

  describe('updateSteps', () => {
    it('updates steps jsonb column', async () => {
      queryMock.mockResolvedValueOnce({ rows: [] });
      const newSteps = [{ description: 'updated step', status: 'running' as const }];

      await pipelineService.updateSteps('test-id', newSteps);

      expect(queryMock).toHaveBeenCalledTimes(1);
      expect(queryMock).toHaveBeenCalledWith(
        expect.stringContaining('UPDATE pipelines SET steps = $1 WHERE id = $2'),
        [JSON.stringify(newSteps), 'test-id']
      );
    });
  });
});
