import { renderHook, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { toast } from 'sonner';
import * as api from '@/lib/api/notifications';
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
      mutations: {
        retry: false,
      },
    },
  });

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={createTestQueryClient()}>{children}</QueryClientProvider>
);

describe('useNotifications hooks', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('useChannels', () => {
    it('calls listChannels', async () => {
      const mockChannels = [{ id: '1', name: 'Channel 1' }];
      vi.mocked(api.listChannels).mockResolvedValue(mockChannels as unknown as api.NotificationChannel[]);

      const { result } = renderHook(() => useChannels(), { wrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(api.listChannels).toHaveBeenCalled();
      expect(result.current.data).toEqual(mockChannels);
    });
  });

  describe('useRoutingRules', () => {
    it('calls listRoutingRules', async () => {
      const mockRules = [{ id: '1', eventType: 'test' }];
      vi.mocked(api.listRoutingRules).mockResolvedValue(mockRules as unknown as api.RoutingRule[]);

      const { result } = renderHook(() => useRoutingRules(), { wrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(api.listRoutingRules).toHaveBeenCalled();
      expect(result.current.data).toEqual(mockRules);
    });
  });

  describe('useCreateChannel', () => {
    it('calls createChannel and shows success toast on success', async () => {
      const payload = { name: 'New Channel', type: 'webhook', config: {} };
      vi.mocked(api.createChannel).mockResolvedValue({ id: '1', ...payload } as unknown as api.NotificationChannel);

      const { result } = renderHook(() => useCreateChannel(), { wrapper });

      await result.current.mutateAsync(payload);

      expect(api.createChannel).toHaveBeenCalledWith(payload);
      expect(toast.success).toHaveBeenCalledWith('Channel created');
    });

    it('shows error toast on failure', async () => {
      const error = new Error('Failed');
      vi.mocked(api.createChannel).mockRejectedValue(error);

      const { result } = renderHook(() => useCreateChannel(), { wrapper });

      try {
        await result.current.mutateAsync({ name: 'New Channel', type: 'webhook', config: {} });
      } catch {
        // expected
      }

      expect(toast.error).toHaveBeenCalledWith('Failed to create channel: Failed');
    });
  });

  describe('useUpdateChannel', () => {
    it('calls updateChannel and shows success toast on success', async () => {
      const data = { name: 'Updated Name' };
      vi.mocked(api.updateChannel).mockResolvedValue({ id: '1', ...data } as unknown as api.NotificationChannel);

      const { result } = renderHook(() => useUpdateChannel(), { wrapper });

      await result.current.mutateAsync({ id: '1', data });

      expect(api.updateChannel).toHaveBeenCalledWith('1', data);
      expect(toast.success).toHaveBeenCalledWith('Channel updated');
    });
  });

  describe('useDeleteChannel', () => {
    it('calls deleteChannel and shows success toast on success', async () => {
      vi.mocked(api.deleteChannel).mockResolvedValue({ ok: true });

      const { result } = renderHook(() => useDeleteChannel(), { wrapper });

      await result.current.mutateAsync('1');

      expect(api.deleteChannel).toHaveBeenCalledWith('1');
      expect(toast.success).toHaveBeenCalledWith('Channel deleted');
    });
  });

  describe('useTestChannel', () => {
    it('calls testChannel and shows success toast when ok is true', async () => {
      vi.mocked(api.testChannel).mockResolvedValue({ ok: true });

      const { result } = renderHook(() => useTestChannel(), { wrapper });

      await result.current.mutateAsync('1');

      expect(api.testChannel).toHaveBeenCalledWith('1');
      expect(toast.success).toHaveBeenCalledWith('Test notification delivered');
    });

    it('shows error toast when ok is false', async () => {
      vi.mocked(api.testChannel).mockResolvedValue({ ok: false, error: 'Connection refused' });

      const { result } = renderHook(() => useTestChannel(), { wrapper });

      await result.current.mutateAsync('1');

      expect(toast.error).toHaveBeenCalledWith('Delivery failed: Connection refused');
    });
  });

  describe('useCreateRoutingRule', () => {
    it('calls createRoutingRule and shows success toast on success', async () => {
      const payload = { eventType: 'test', channelIds: ['1'] };
      vi.mocked(api.createRoutingRule).mockResolvedValue({ id: '1', ...payload } as unknown as api.RoutingRule);

      const { result } = renderHook(() => useCreateRoutingRule(), { wrapper });

      await result.current.mutateAsync(payload);

      expect(api.createRoutingRule).toHaveBeenCalledWith(payload);
      expect(toast.success).toHaveBeenCalledWith('Routing rule created');
    });
  });

  describe('useUpdateRoutingRule', () => {
    it('calls updateRoutingRule and shows success toast on success', async () => {
      const data = { eventType: 'updated' };
      vi.mocked(api.updateRoutingRule).mockResolvedValue({ id: '1', ...data } as unknown as api.RoutingRule);

      const { result } = renderHook(() => useUpdateRoutingRule(), { wrapper });

      await result.current.mutateAsync({ id: '1', data });

      expect(api.updateRoutingRule).toHaveBeenCalledWith('1', data);
      expect(toast.success).toHaveBeenCalledWith('Routing rule updated');
    });
  });

  describe('useDeleteRoutingRule', () => {
    it('calls deleteRoutingRule and shows success toast on success', async () => {
      vi.mocked(api.deleteRoutingRule).mockResolvedValue({ ok: true });

      const { result } = renderHook(() => useDeleteRoutingRule(), { wrapper });

      await result.current.mutateAsync('1');

      expect(api.deleteRoutingRule).toHaveBeenCalledWith('1');
      expect(toast.success).toHaveBeenCalledWith('Routing rule deleted');
    });
  });
});
