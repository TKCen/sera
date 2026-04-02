import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider, useQueryClient } from '@tanstack/react-query';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import * as schedulesApi from '@/lib/api/schedules';
import {
  useSchedules,
  useCreateSchedule,
  useUpdateSchedule,
  useDeleteSchedule,
  useTriggerSchedule,
  useScheduleRuns,
  useScheduleRunsBySchedule,
} from './useSchedules';
import React from 'react';

vi.mock('@/lib/api/schedules');
vi.mock('@tanstack/react-query', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@tanstack/react-query')>();
  return {
    ...actual,
    useQueryClient: vi.fn(),
  };
});

const createTestQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

const wrapper = ({ children }: { children: React.ReactNode }) => {
  const queryClient = createTestQueryClient();
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
};

describe('useSchedules hooks', () => {
  const mockInvalidateQueries = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(useQueryClient).mockReturnValue({
      invalidateQueries: mockInvalidateQueries,
    } as any);
  });

  it('should be defined', () => {
    expect(useSchedules).toBeDefined();
    expect(useCreateSchedule).toBeDefined();
    expect(useUpdateSchedule).toBeDefined();
    expect(useDeleteSchedule).toBeDefined();
    expect(useTriggerSchedule).toBeDefined();
    expect(useScheduleRuns).toBeDefined();
    expect(useScheduleRunsBySchedule).toBeDefined();
  });

  describe('useSchedules', () => {
    it('should fetch schedules', async () => {
      const mockSchedules = [{ id: '1', name: 'Test Schedule' }];
      vi.mocked(schedulesApi.listSchedules).mockResolvedValue(mockSchedules as any);

      const { result } = renderHook(() => useSchedules(), { wrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toEqual(mockSchedules);
      expect(schedulesApi.listSchedules).toHaveBeenCalledWith({});
    });

    it('should fetch schedules with params', async () => {
      const params = { agentName: 'test-agent', status: 'active' };
      vi.mocked(schedulesApi.listSchedules).mockResolvedValue([]);

      const { result } = renderHook(() => useSchedules(params), { wrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(schedulesApi.listSchedules).toHaveBeenCalledWith(params);
    });
  });

  describe('useCreateSchedule', () => {
    it('should create a schedule and invalidate queries', async () => {
      const newSchedule = {
        agentName: 'agent',
        name: 'new',
        type: 'cron',
        expression: '* * * * *',
      };
      vi.mocked(schedulesApi.createSchedule).mockResolvedValue({ id: '1', ...newSchedule } as any);

      const { result } = renderHook(() => useCreateSchedule(), { wrapper });

      await result.current.mutateAsync(newSchedule as any);

      expect(schedulesApi.createSchedule).toHaveBeenCalledWith(newSchedule);
      expect(mockInvalidateQueries).toHaveBeenCalledWith({ queryKey: ['schedules'] });
    });

    it('should handle creation error', async () => {
      const error = new Error('Creation failed');
      vi.mocked(schedulesApi.createSchedule).mockRejectedValue(error);

      const { result } = renderHook(() => useCreateSchedule(), { wrapper });

      try {
        await result.current.mutateAsync({} as any);
      } catch (e) {
        // Expected
      }

      await waitFor(() => expect(result.current.isError).toBe(true));
      expect(result.current.error).toEqual(error);
    });
  });

  describe('useUpdateSchedule', () => {
    it('should update a schedule and invalidate queries', async () => {
      const id = '1';
      const updateData = { name: 'Updated' };
      vi.mocked(schedulesApi.updateSchedule).mockResolvedValue({ id, ...updateData } as any);

      const { result } = renderHook(() => useUpdateSchedule(), { wrapper });

      await result.current.mutateAsync({ id, data: updateData });

      expect(schedulesApi.updateSchedule).toHaveBeenCalledWith(id, updateData);
      expect(mockInvalidateQueries).toHaveBeenCalledWith({ queryKey: ['schedules'] });
    });
  });

  describe('useDeleteSchedule', () => {
    it('should delete a schedule and invalidate queries', async () => {
      const id = '1';
      vi.mocked(schedulesApi.deleteSchedule).mockResolvedValue({ success: true });

      const { result } = renderHook(() => useDeleteSchedule(), { wrapper });

      await result.current.mutateAsync(id);

      expect(schedulesApi.deleteSchedule).toHaveBeenCalledWith(id);
      expect(mockInvalidateQueries).toHaveBeenCalledWith({ queryKey: ['schedules'] });
    });
  });

  describe('useTriggerSchedule', () => {
    it('should trigger a schedule and invalidate queries', async () => {
      const id = '1';
      vi.mocked(schedulesApi.triggerSchedule).mockResolvedValue({ status: 'triggered' });

      const { result } = renderHook(() => useTriggerSchedule(), { wrapper });

      await result.current.mutateAsync(id);

      expect(schedulesApi.triggerSchedule).toHaveBeenCalledWith(id);
      expect(mockInvalidateQueries).toHaveBeenCalledWith({ queryKey: ['schedules'] });
    });
  });

  describe('useScheduleRuns', () => {
    it('should fetch schedule runs', async () => {
      const mockRuns = [{ taskId: 'task-1', scheduleId: '1' }];
      vi.mocked(schedulesApi.listScheduleRuns).mockResolvedValue(mockRuns as any);

      const { result } = renderHook(() => useScheduleRuns(), { wrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toEqual(mockRuns);
      expect(schedulesApi.listScheduleRuns).toHaveBeenCalledWith({});
    });
  });

  describe('useScheduleRunsBySchedule', () => {
    it('should fetch schedule runs for a specific schedule', async () => {
      const id = '1';
      const mockRuns = [{ taskId: 'task-1', scheduleId: id }];
      vi.mocked(schedulesApi.getScheduleRuns).mockResolvedValue(mockRuns as any);

      const { result } = renderHook(() => useScheduleRunsBySchedule(id), { wrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toEqual(mockRuns);
      expect(schedulesApi.getScheduleRuns).toHaveBeenCalledWith(id, {});
    });
  });
});
