import { request } from './client';
import type { Schedule, ScheduleRun } from './types';

export function listSchedules(
  params: { agentName?: string; status?: string } = {}
): Promise<Schedule[]> {
  const q = new URLSearchParams();
  if (params.agentName) q.set('agentName', params.agentName);
  if (params.status) q.set('status', params.status);
  const qs = q.toString();
  return request<Schedule[]>(`/schedules${qs ? `?${qs}` : ''}`);
}

export function createSchedule(
  data: Omit<
    Schedule,
    'id' | 'source' | 'lastRunAt' | 'lastRunStatus' | 'lastRunOutput' | 'nextRunAt'
  >
): Promise<Schedule> {
  return request<Schedule>('/schedules', {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export function updateSchedule(
  id: string,
  data: Partial<Pick<Schedule, 'expression' | 'taskPrompt' | 'status' | 'name'>>
): Promise<Schedule> {
  // Backend expects 'task' not 'taskPrompt'
  const { taskPrompt, ...rest } = data;
  const payload = taskPrompt !== undefined ? { ...rest, task: taskPrompt } : rest;
  return request<Schedule>(`/schedules/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    body: JSON.stringify(payload),
  });
}

export function deleteSchedule(id: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/schedules/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

export function triggerSchedule(id: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/schedules/${encodeURIComponent(id)}/trigger`, {
    method: 'POST',
  });
}

export function listScheduleRuns(
  params: { category?: string; scheduleId?: string; agentId?: string; limit?: number } = {}
): Promise<ScheduleRun[]> {
  const q = new URLSearchParams();
  if (params.category) q.set('category', params.category);
  if (params.scheduleId) q.set('scheduleId', params.scheduleId);
  if (params.agentId) q.set('agentId', params.agentId);
  if (params.limit) q.set('limit', String(params.limit));
  const qs = q.toString();
  return request<ScheduleRun[]>(`/schedules/runs${qs ? `?${qs}` : ''}`);
}

export function getScheduleRuns(
  id: string,
  params: { limit?: number } = {}
): Promise<ScheduleRun[]> {
  const q = new URLSearchParams();
  if (params.limit) q.set('limit', String(params.limit));
  const qs = q.toString();
  return request<ScheduleRun[]>(`/schedules/${encodeURIComponent(id)}/runs${qs ? `?${qs}` : ''}`);
}
