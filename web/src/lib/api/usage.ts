import { request } from './client';
import type { UsageResponse } from './types';

export function getUsage(params: {
  groupBy?: 'agent' | 'model';
  from?: string;
  to?: string;
}): Promise<UsageResponse> {
  const q = new URLSearchParams();
  if (params.groupBy) q.set('groupBy', params.groupBy);
  if (params.from) q.set('from', params.from);
  if (params.to) q.set('to', params.to);
  const qs = q.toString();
  return request<UsageResponse>(`/metering/usage${qs ? `?${qs}` : ''}`);
}

export function getAgentBudget(agentName: string): Promise<{
  maxLlmTokensPerHour?: number;
  maxLlmTokensPerDay?: number;
  currentHourTokens: number;
  currentDayTokens: number;
}> {
  return request(`/agents/${encodeURIComponent(agentName)}/budget`);
}

export function patchAgentBudget(
  agentName: string,
  budget: { maxLlmTokensPerHour?: number | null; maxLlmTokensPerDay?: number | null }
): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/agents/${encodeURIComponent(agentName)}/budget`, {
    method: 'PATCH',
    body: JSON.stringify(budget),
  });
}

export function resetAgentBudget(agentName: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/agents/${encodeURIComponent(agentName)}/budget/reset`, {
    method: 'POST',
  });
}
