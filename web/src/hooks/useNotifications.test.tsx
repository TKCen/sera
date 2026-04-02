import { renderHook, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  useChannels,
  useRoutingRules,
  useCreateChannel,
  useUpdateChannel,
  useDeleteChannel,
  useTestChannel,
  useCreateRoutingRule,
  useUpdateRoutingRule,
  useDeleteRoutingRule,
} from './useNotifications';
import * as api from '@/lib/api/notifications';
import { toast } from 'sonner';
import React from 'react';

vi.mock('@/lib/api/notifications', () => ({
  listChannels: vi.fn(),
  createChannel: vi.fn(),
  updateChannel: vi.fn(),
  deleteChannel: vi.fn(),
  testChannel: vi.fn(),
  listRoutingRules: vi.fn(),
  createRoutingRule: vi.fn(),
  updateRoutingRule: vi.fn(),
  deleteRoutingRule: vi.fn(),
}));

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

const createTestQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

describe('useNotifications hooks', () => {
  let queryClient: QueryClient;
  let wrapper: React.FC<{ children: React.ReactNode }>;

  beforeEach(() => {
    vi.clearAllMocks();
    queryClient = createTestQueryClient();
    wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  });

  describe('useChannels', () => {
    it('fetches channels successfully', async () => {
      const mockChannels = [{ id: '1', name: 'Channel 1' }];
      vi.mocked(api.listChannels).mockResolvedValue(mockChannels as any);

      const { result } = renderHook(() => useChannels(), { wrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toEqual(mockChannels);
      expect(api.listChannels).toHaveBeenCalled();
    });
  });

  describe('useRoutingRules', () => {
    it('fetches routing rules successfully', async () => {
      const mockRules = [{ id: '1', eventType: 'test' }];
      vi.mocked(api.listRoutingRules).mockResolvedValue(mockRules as any);

      const { result } = renderHook(() => useRoutingRules(), { wrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toEqual(mockRules);
      expect(api.listRoutingRules).toHaveBeenCalled();
    });
  });

  describe('useCreateChannel', () => {
    it('creates a channel and invalidates cache', async () => {
      const payload = { name: 'New Channel', type: 'webhook', config: {} };
      vi.mocked(api.createChannel).mockResolvedValue({ id: '2', ...payload } as any);
      const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

      const { result } = renderHook(() => useCreateChannel(), { wrapper });
      result.current.mutate(payload);

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(api.createChannel).toHaveBeenCalledWith(payload);
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['notification-channels'] });
      expect(toast.success).toHaveBeenCalledWith('Channel created');
    });

    it('shows error toast on failure', async () => {
      vi.mocked(api.createChannel).mockRejectedValue(new Error('Failed'));
      const { result } = renderHook(() => useCreateChannel(), { wrapper });
      result.current.mutate({ name: 'Fail' } as any);

      await waitFor(() => expect(result.current.isError).toBe(true));
      expect(toast.error).toHaveBeenCalledWith('Failed to create channel: Failed');
    });
  });

  describe('useUpdateChannel', () => {
    it('updates a channel and invalidates cache', async () => {
      const id = '1';
      const data = { name: 'Updated Name' };
      vi.mocked(api.updateChannel).mockResolvedValue({ id, ...data } as any);
      const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

      const { result } = renderHook(() => useUpdateChannel(), { wrapper });
      result.current.mutate({ id, data });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(api.updateChannel).toHaveBeenCalledWith(id, data);
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['notification-channels'] });
      expect(toast.success).toHaveBeenCalledWith('Channel updated');
    });
  });

  describe('useDeleteChannel', () => {
    it('deletes a channel and invalidates caches', async () => {
      const id = '1';
      vi.mocked(api.deleteChannel).mockResolvedValue({ ok: true });
      const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

      const { result } = renderHook(() => useDeleteChannel(), { wrapper });
      result.current.mutate(id);

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(api.deleteChannel).toHaveBeenCalledWith(id);
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['notification-channels'] });
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['notification-routing-rules'] });
      expect(toast.success).toHaveBeenCalledWith('Channel deleted');
    });
  });

  describe('useTestChannel', () => {
    it('tests a channel successfully', async () => {
      const id = '1';
      vi.mocked(api.testChannel).mockResolvedValue({ ok: true });

      const { result } = renderHook(() => useTestChannel(), { wrapper });
      result.current.mutate(id);

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(api.testChannel).toHaveBeenCalledWith(id);
      expect(toast.success).toHaveBeenCalledWith('Test notification delivered');
    });

    it('shows error when test delivery fails', async () => {
      const id = '1';
      vi.mocked(api.testChannel).mockResolvedValue({ ok: false, error: 'connection refused' });

      const { result } = renderHook(() => useTestChannel(), { wrapper });
      result.current.mutate(id);

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(toast.error).toHaveBeenCalledWith('Delivery failed: connection refused');
    });
  });

  describe('useCreateRoutingRule', () => {
    it('creates a routing rule and invalidates cache', async () => {
      const payload = { eventType: 'test', channelIds: ['1'] };
      vi.mocked(api.createRoutingRule).mockResolvedValue({ id: 'rule-1', ...payload } as any);
      const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

      const { result } = renderHook(() => useCreateRoutingRule(), { wrapper });
      result.current.mutate(payload);

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(api.createRoutingRule).toHaveBeenCalledWith(payload);
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['notification-routing-rules'] });
      expect(toast.success).toHaveBeenCalledWith('Routing rule created');
    });
  });

  describe('useUpdateRoutingRule', () => {
    it('updates a routing rule and invalidates cache', async () => {
      const id = 'rule-1';
      const data = { eventType: 'updated' };
      vi.mocked(api.updateRoutingRule).mockResolvedValue({ id, ...data } as any);
      const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

      const { result } = renderHook(() => useUpdateRoutingRule(), { wrapper });
      result.current.mutate({ id, data });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(api.updateRoutingRule).toHaveBeenCalledWith(id, data);
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['notification-routing-rules'] });
      expect(toast.success).toHaveBeenCalledWith('Routing rule updated');
    });
  });

  describe('useDeleteRoutingRule', () => {
    it('deletes a routing rule and invalidates cache', async () => {
      const id = 'rule-1';
      vi.mocked(api.deleteRoutingRule).mockResolvedValue({ ok: true });
      const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

      const { result } = renderHook(() => useDeleteRoutingRule(), { wrapper });
      result.current.mutate(id);

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(api.deleteRoutingRule).toHaveBeenCalledWith(id);
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['notification-routing-rules'] });
      expect(toast.success).toHaveBeenCalledWith('Routing rule deleted');
    });
  });
});
