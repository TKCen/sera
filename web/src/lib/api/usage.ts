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

export async function getAgentBudget(agentId: string): Promise<{
  maxLlmTokensPerHour?: number;
  maxLlmTokensPerDay?: number;
  currentHourTokens: number;
  currentDayTokens: number;
}> {
  // Backend returns { agentId, allowed, hourlyUsed, hourlyQuota, dailyUsed, dailyQuota }
  const data = await request<{
    allowed: boolean;
    hourlyUsed: number;
    hourlyQuota: number;
    dailyUsed: number;
    dailyQuota: number;
  }>(`/budget/agents/${encodeURIComponent(agentId)}/budget`);
  return {
    maxLlmTokensPerHour: data.hourlyQuota,
    maxLlmTokensPerDay: data.dailyQuota,
    currentHourTokens: data.hourlyUsed,
    currentDayTokens: data.dailyUsed,
  };
}

export function patchAgentBudget(
  agentId: string,
  budget: { maxLlmTokensPerHour?: number | null; maxLlmTokensPerDay?: number | null }
): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/budget/agents/${encodeURIComponent(agentId)}/budget`, {
    method: 'PATCH',
    body: JSON.stringify(budget),
  });
}

export function resetAgentBudget(agentId: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(
    `/budget/agents/${encodeURIComponent(agentId)}/budget/reset`,
    {
      method: 'POST',
    }
  );
}
