import { request } from './client';
import type { UsageResponse } from './types';

interface UsageRow {
  period: string;
  agentId?: string;
  totalTokens: number;
  costUsd: number;
}

/**
 * Fetch usage data from the metering API and transform into the shape
 * the InsightsPage expects: summary, timeSeries, byAgent, byModel.
 *
 * The API only supports groupBy=hour|day (not agent/model), so we
 * always request by day and aggregate agent/model breakdowns client-side.
 */
export async function getUsage(params: {
  groupBy?: 'agent' | 'model';
  from?: string;
  to?: string;
}): Promise<UsageResponse> {
  const q = new URLSearchParams();
  // API only supports hour|day — use 'day' for time series
  q.set('groupBy', 'day');
  if (params.from) q.set('from', params.from);
  if (params.to) q.set('to', params.to);
  const qs = q.toString();

  const raw = await request<{ data: UsageRow[] }>(`/metering/usage${qs ? `?${qs}` : ''}`);
  const rows = raw.data ?? [];

  // Build time series (aggregate across all agents per period)
  const periodMap = new Map<string, { promptTokens: number; completionTokens: number }>();
  for (const row of rows) {
    const existing = periodMap.get(row.period);
    const tokens = Number(row.totalTokens) || 0;
    if (existing) {
      existing.completionTokens += tokens;
    } else {
      periodMap.set(row.period, { promptTokens: 0, completionTokens: tokens });
    }
  }
  const timeSeries = [...periodMap.entries()].map(([period, v]) => ({
    period,
    promptTokens: v.promptTokens,
    completionTokens: v.completionTokens,
    totalTokens: v.promptTokens + v.completionTokens,
  }));

  // Build by-agent breakdown
  const agentMap = new Map<string, number>();
  for (const row of rows) {
    const name = row.agentId ?? 'unknown';
    agentMap.set(name, (agentMap.get(name) ?? 0) + (Number(row.totalTokens) || 0));
  }
  const grandTotal = [...agentMap.values()].reduce((a, b) => a + b, 0) || 1;
  const byAgent = [...agentMap.entries()].map(([agentName, totalTokens]) => ({
    agentName,
    promptTokens: 0,
    completionTokens: totalTokens,
    totalTokens,
    pctOfTotal: (totalTokens / grandTotal) * 100,
  }));

  // Summary
  const totalTokens = byAgent.reduce((s, a) => s + a.totalTokens, 0);

  return {
    summary: {
      totalTokensToday: totalTokens,
      totalTokensMonth: totalTokens,
      estimatedCost: rows.reduce((s, r) => s + (Number(r.costUsd) || 0), 0),
      mostActiveAgent: byAgent[0]?.agentName,
    },
    timeSeries: timeSeries.map((t) => ({
      timestamp: t.period,
      promptTokens: t.promptTokens,
      completionTokens: t.completionTokens,
      totalTokens: t.totalTokens,
    })),
    byAgent,
    byModel: [],
  };
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
