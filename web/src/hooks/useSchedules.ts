import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as schedulesApi from '@/lib/api/schedules';
import type { Schedule } from '@/lib/api/types';

export const scheduleKeys = {
  all: (params?: object) => ['schedules', params] as const,
};

export function useSchedules(params: { agentName?: string; status?: string } = {}) {
  return useQuery({
    queryKey: scheduleKeys.all(params),
    queryFn: () => schedulesApi.listSchedules(params),
    refetchInterval: 30_000,
  });
}

export function useCreateSchedule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (
      data: Omit<
        Schedule,
        'id' | 'source' | 'lastRunAt' | 'lastRunStatus' | 'lastRunOutput' | 'nextRunAt'
      >
    ) => schedulesApi.createSchedule(data),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['schedules'] });
    },
  });
}

export function useUpdateSchedule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      data,
    }: {
      id: string;
      data: Partial<Pick<Schedule, 'expression' | 'taskPrompt' | 'status' | 'name'>>;
    }) => schedulesApi.updateSchedule(id, data),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['schedules'] });
    },
  });
}

export function useDeleteSchedule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => schedulesApi.deleteSchedule(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['schedules'] });
    },
  });
}

export function useTriggerSchedule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => schedulesApi.triggerSchedule(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['schedules'] });
    },
  });
}

export const scheduleRunKeys = {
  all: (params?: object) => ['schedule-runs', params] as const,
  bySchedule: (id: string, params?: object) => ['schedule-runs', 'schedule', id, params] as const,
};

export function useScheduleRuns(
  params: { category?: string; scheduleId?: string; agentId?: string; limit?: number } = {}
) {
  return useQuery({
    queryKey: scheduleRunKeys.all(params),
    queryFn: () => schedulesApi.listScheduleRuns(params),
    refetchInterval: 30_000,
  });
}

export function useScheduleRunsBySchedule(id: string, params: { limit?: number } = {}) {
  return useQuery({
    queryKey: scheduleRunKeys.bySchedule(id, params),
    queryFn: () => schedulesApi.getScheduleRuns(id, params),
    enabled: !!id,
    refetchInterval: 30_000,
  });
}
