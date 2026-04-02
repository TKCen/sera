import { describe, it, expect, vi, beforeEach } from 'vitest';
import { registerMCPServer, unregisterMCPServer } from './mcp';
import { request } from './client';

vi.mock('./client', () => ({
  request: vi.fn(),
}));

describe('mcp api', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('registerMCPServer', () => {
    it('should call request with correct parameters', async () => {
      const manifest = { name: 'test-server', version: '1.0.0' };
      const mockResponse = { message: 'Server registered' };
      vi.mocked(request).mockResolvedValueOnce(mockResponse);

      const result = await registerMCPServer(manifest);

      expect(request).toHaveBeenCalledWith('/mcp-servers', {
        method: 'POST',
        body: JSON.stringify(manifest),
      });
      expect(result).toEqual(mockResponse);
    });
  });

  describe('unregisterMCPServer', () => {
    it('should call request with correct parameters and encoded name', async () => {
      const serverName = 'test server';
      const mockResponse = { message: 'Server unregistered' };
      vi.mocked(request).mockResolvedValueOnce(mockResponse);

      const result = await unregisterMCPServer(serverName);

      expect(request).toHaveBeenCalledWith(`/mcp-servers/${encodeURIComponent(serverName)}`, {
        method: 'DELETE',
      });
      expect(result).toEqual(mockResponse);
    });
  });
});
