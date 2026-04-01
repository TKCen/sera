import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import express from 'express';
import { createMemoryRouter } from './memory.js';

// Mock dependencies
const mockMemoryManager = {
  deleteEntry: vi.fn(),
};

const mockScopedStore = {
  delete: vi.fn(),
  deleteArchive: vi.fn(),
  listAgentIds: vi.fn().mockResolvedValue([]),
};

const mockVectorService = {
  delete: vi.fn(),
};

// We need to mock the constructor of ScopedMemoryBlockStore and VectorService inside the router file
// or just mock the entire modules if we want to control the instances created inside createMemoryRouter.
// Since createMemoryRouter instantiates them internally, we'll mock the modules.

vi.mock('../memory/blocks/ScopedMemoryBlockStore.js', () => {
  return {
    ScopedMemoryBlockStore: vi.fn().mockImplementation(function() {
      return mockScopedStore;
    }),
  };
});

vi.mock('../services/vector.service.js', () => {
  return {
    VectorService: vi.fn().mockImplementation(function() {
      return mockVectorService;
    }),
  };
});

describe('Memory Routes DELETE', () => {
  let app: express.Express;

  beforeEach(() => {
    vi.clearAllMocks();
    app = express();
    app.use(express.json());
    app.use('/api/memory', createMemoryRouter(mockMemoryManager as any));
  });

  describe('DELETE /api/memory/entries/:id', () => {
    it('returns 204 on successful deletion', async () => {
      mockMemoryManager.deleteEntry.mockResolvedValue(true);
      const res = await request(app).delete('/api/memory/entries/entry-123');
      expect(res.status).toBe(204);
      expect(mockMemoryManager.deleteEntry).toHaveBeenCalledWith('entry-123');
    });

    it('returns 404 when entry not found', async () => {
      mockMemoryManager.deleteEntry.mockResolvedValue(false);
      const res = await request(app).delete('/api/memory/entries/non-existent');
      expect(res.status).toBe(404);
    });

    it('returns 500 on error', async () => {
      mockMemoryManager.deleteEntry.mockRejectedValue(new Error('Store error'));
      const res = await request(app).delete('/api/memory/entries/error-id');
      expect(res.status).toBe(500);
      expect(res.body.error).toBe('Store error');
    });
  });

  describe('DELETE /api/memory/:agentId/blocks/:id', () => {
    const agentId = 'agent-456';
    const blockId = 'block-789';

    it('returns 204 when deleted from active store', async () => {
      mockScopedStore.delete.mockResolvedValue(true);
      const res = await request(app).delete(`/api/memory/${agentId}/blocks/${blockId}`);

      expect(res.status).toBe(204);
      expect(mockScopedStore.delete).toHaveBeenCalledWith(agentId, blockId);
      expect(mockVectorService.delete).toHaveBeenCalledWith(blockId, `personal:${agentId}`);
    });

    it('returns 204 when deleted from archive store', async () => {
      mockScopedStore.delete.mockResolvedValue(false);
      mockScopedStore.deleteArchive.mockResolvedValue(true);
      const res = await request(app).delete(`/api/memory/${agentId}/blocks/${blockId}`);

      expect(res.status).toBe(204);
      expect(mockScopedStore.delete).toHaveBeenCalledWith(agentId, blockId);
      expect(mockScopedStore.deleteArchive).toHaveBeenCalledWith(agentId, blockId);
      expect(mockVectorService.delete).toHaveBeenCalledWith(blockId, `personal:${agentId}`);
    });

    it('handles global namespace correctly', async () => {
      mockScopedStore.delete.mockResolvedValue(true);
      const res = await request(app).delete(`/api/memory/global/blocks/${blockId}`);

      expect(res.status).toBe(204);
      expect(mockVectorService.delete).toHaveBeenCalledWith(blockId, 'global');
    });

    it('handles circle namespace correctly', async () => {
      const circleId = 'circle:123';
      mockScopedStore.delete.mockResolvedValue(true);
      const res = await request(app).delete(`/api/memory/${circleId}/blocks/${blockId}`);

      expect(res.status).toBe(204);
      expect(mockVectorService.delete).toHaveBeenCalledWith(blockId, circleId);
    });

    it('returns 404 when not found in active or archive', async () => {
      mockScopedStore.delete.mockResolvedValue(false);
      mockScopedStore.deleteArchive.mockResolvedValue(false);
      const res = await request(app).delete(`/api/memory/${agentId}/blocks/${blockId}`);

      expect(res.status).toBe(404);
      expect(mockVectorService.delete).not.toHaveBeenCalled();
    });

    it('returns 500 on error', async () => {
      mockScopedStore.delete.mockRejectedValue(new Error('File system error'));
      const res = await request(app).delete(`/api/memory/${agentId}/blocks/${blockId}`);
      expect(res.status).toBe(500);
    });
  });
});
