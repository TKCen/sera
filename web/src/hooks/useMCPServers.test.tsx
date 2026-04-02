import { renderHook, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useRegisterMCPServer, useUnregisterMCPServer } from './useMCPServers';
import * as mcpApi from '@/lib/api/mcp';
import React from 'react';

vi.mock('@/lib/api/mcp', () => ({
  registerMCPServer: vi.fn(),
  unregisterMCPServer: vi.fn(),
}));

const createTestQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
      mutations: {
        retry: false,
      },
    },
  });

describe('useMCPServers hooks', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('useRegisterMCPServer', () => {
    it('should call registerMCPServer and invalidate tools query on success', async () => {
      const manifest = { name: 'test-server' };
      vi.mocked(mcpApi.registerMCPServer).mockResolvedValueOnce({ message: 'ok' });

      const queryClient = createTestQueryClient();
      const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');
      const wrapper = ({ children }: { children: React.ReactNode }) => (
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      );

      const { result } = renderHook(() => useRegisterMCPServer(), { wrapper });

      result.current.mutate(manifest);

      await waitFor(() => expect(result.current.isSuccess).toBe(true));

      expect(mcpApi.registerMCPServer).toHaveBeenCalledWith(manifest);
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['tools'] });
    });

    it('should handle errors from registerMCPServer', async () => {
      const manifest = { name: 'test-server' };
      const error = new Error('Failed to register');
      vi.mocked(mcpApi.registerMCPServer).mockRejectedValueOnce(error);

      const queryClient = createTestQueryClient();
      const wrapper = ({ children }: { children: React.ReactNode }) => (
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      );

      const { result } = renderHook(() => useRegisterMCPServer(), { wrapper });

      result.current.mutate(manifest);

      await waitFor(() => expect(result.current.isError).toBe(true));
      expect(result.current.error).toBe(error);
    });
  });

  describe('useUnregisterMCPServer', () => {
    it('should call unregisterMCPServer and invalidate tools query on success', async () => {
      const serverName = 'test-server';
      vi.mocked(mcpApi.unregisterMCPServer).mockResolvedValueOnce({ message: 'ok' });

      const queryClient = createTestQueryClient();
      const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');
      const wrapper = ({ children }: { children: React.ReactNode }) => (
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      );

      const { result } = renderHook(() => useUnregisterMCPServer(), { wrapper });

      result.current.mutate(serverName);

      await waitFor(() => expect(result.current.isSuccess).toBe(true));

      expect(mcpApi.unregisterMCPServer).toHaveBeenCalledWith(serverName);
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['tools'] });
    });

    it('should handle errors from unregisterMCPServer', async () => {
      const serverName = 'test-server';
      const error = new Error('Failed to unregister');
      vi.mocked(mcpApi.unregisterMCPServer).mockRejectedValueOnce(error);

      const queryClient = createTestQueryClient();
      const wrapper = ({ children }: { children: React.ReactNode }) => (
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      );

      const { result } = renderHook(() => useUnregisterMCPServer(), { wrapper });

      result.current.mutate(serverName);

      await waitFor(() => expect(result.current.isError).toBe(true));
      expect(result.current.error).toBe(error);
    });
  });
});
