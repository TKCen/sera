import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import express from 'express';
import { createMemoryRouter } from './memory.js';
import type { MemoryManager } from '../memory/manager.js';

// Mock dependencies
const mockVectorService = {
  delete: vi.fn(),
  search: vi.fn(),
  getCollectionInfo: vi.fn(),
};

const mockMemoryManager = {
  deleteEntry: vi.fn(),
  vectorService: mockVectorService,
};

const mockScopedStore = {
  delete: vi.fn(),
  deleteArchive: vi.fn(),
  listAgentIds: vi.fn().mockResolvedValue([]),
};

// We need to mock the constructor of ScopedMemoryBlockStore inside the router file
// or just mock the entire modules if we want to control the instances created inside createMemoryRouter.
// Since createMemoryRouter instantiates them internally, we'll mock the modules.

vi.mock('../memory/blocks/ScopedMemoryBlockStore.js', () => {
  return {
    ScopedMemoryBlockStore: vi.fn().mockImplementation(function () {
      return mockScopedStore;
    }),
  };
});

describe('Memory Routes DELETE', () => {
  let app: express.Express;

  beforeEach(() => {
    vi.clearAllMocks();
    app = express();
    app.use(express.json());
    app.use('/api/memory', createMemoryRouter(mockMemoryManager as unknown as MemoryManager));
  });

  describe('DELETE /api/memory/entries/:id', () => {
    it('returns 204 on successful deletion', async () => {
      vi.mocked(mockMemoryManager.deleteEntry).mockResolvedValue(true);
      const res = await request(app).delete('/api/memory/entries/entry-123');
      expect(res.status).toBe(204);
      expect(vi.mocked(mockMemoryManager.deleteEntry)).toHaveBeenCalledWith('entry-123');
    });

    it('returns 404 when entry not found', async () => {
      vi.mocked(mockMemoryManager.deleteEntry).mockResolvedValue(false);
      const res = await request(app).delete('/api/memory/entries/non-existent');
      expect(res.status).toBe(404);
    });

    it('returns 500 on error', async () => {
      vi.mocked(mockMemoryManager.deleteEntry).mockRejectedValue(new Error('Store error'));
      const res = await request(app).delete('/api/memory/entries/error-id');
      expect(res.status).toBe(500);
      expect(res.body.error).toBe('Store error');
    });
  });

  describe('DELETE /api/memory/:agentId/blocks/:id', () => {
    const agentId = 'agent-456';
    const blockId = 'block-789';

    it('returns 204 when deleted from active store', async () => {
      vi.mocked(mockScopedStore.delete).mockResolvedValue(true);
      const res = await request(app).delete(`/api/memory/${agentId}/blocks/${blockId}`);

      expect(res.status).toBe(204);
      expect(vi.mocked(mockScopedStore.delete)).toHaveBeenCalledWith(agentId, blockId);
      expect(vi.mocked(mockVectorService.delete)).toHaveBeenCalledWith(
        blockId,
        `personal:${agentId}`
      );
    });

    it('returns 204 when deleted from archive store', async () => {
      vi.mocked(mockScopedStore.delete).mockResolvedValue(false);
      vi.mocked(mockScopedStore.deleteArchive).mockResolvedValue(true);
      const res = await request(app).delete(`/api/memory/${agentId}/blocks/${blockId}`);

      expect(res.status).toBe(204);
      expect(vi.mocked(mockScopedStore.delete)).toHaveBeenCalledWith(agentId, blockId);
      expect(vi.mocked(mockScopedStore.deleteArchive)).toHaveBeenCalledWith(agentId, blockId);
      expect(vi.mocked(mockVectorService.delete)).toHaveBeenCalledWith(
        blockId,
        `personal:${agentId}`
      );
    });

    it('handles global namespace correctly', async () => {
      vi.mocked(mockScopedStore.delete).mockResolvedValue(true);
      const res = await request(app).delete(`/api/memory/global/blocks/${blockId}`);

      expect(res.status).toBe(204);
      expect(vi.mocked(mockVectorService.delete)).toHaveBeenCalledWith(blockId, 'global');
    });

    it('handles circle namespace correctly', async () => {
      const circleId = 'circle:123';
      vi.mocked(mockScopedStore.delete).mockResolvedValue(true);
      const res = await request(app).delete(`/api/memory/${circleId}/blocks/${blockId}`);

      expect(res.status).toBe(204);
      expect(vi.mocked(mockVectorService.delete)).toHaveBeenCalledWith(blockId, circleId);
    });

    it('returns 404 when not found in active or archive', async () => {
      vi.mocked(mockScopedStore.delete).mockResolvedValue(false);
      vi.mocked(mockScopedStore.deleteArchive).mockResolvedValue(false);
      const res = await request(app).delete(`/api/memory/${agentId}/blocks/${blockId}`);

      expect(res.status).toBe(404);
      expect(mockVectorService.delete).not.toHaveBeenCalled();
    });

    it('returns 500 on error', async () => {
      vi.mocked(mockScopedStore.delete).mockRejectedValue(new Error('File system error'));
      const res = await request(app).delete(`/api/memory/${agentId}/blocks/${blockId}`);
      expect(res.status).toBe(500);
    });
  });
});
