import { request } from './client';
import type { Schedule } from './types';

export function listSchedules(params: { agentName?: string; status?: string } = {}): Promise<Schedule[]> {
  const q = new URLSearchParams();
  if (params.agentName) q.set('agentName', params.agentName);
  if (params.status) q.set('status', params.status);
  const qs = q.toString();
  return request<Schedule[]>(`/schedules${qs ? `?${qs}` : ''}`);
}

export function createSchedule(data: Omit<Schedule, 'id' | 'source' | 'lastRunAt' | 'lastRunStatus' | 'lastRunOutput' | 'nextRunAt'>): Promise<Schedule> {
  return request<Schedule>('/schedules', {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export function updateSchedule(
  id: string,
  data: Partial<Pick<Schedule, 'expression' | 'taskPrompt' | 'status' | 'name'>>,
): Promise<Schedule> {
  return request<Schedule>(`/schedules/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    body: JSON.stringify(data),
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
