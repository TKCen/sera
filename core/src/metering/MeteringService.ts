/**
 * MeteringService — tracks token usage and enforces budget quotas.
 *
 * Records per-request LLM token usage to both `token_usage` (budget queries)
 * and `usage_events` (full audit with cost and latency). Checks agents against
 * their quota in `token_quotas` before upstream LLM calls.
 *
 * @see docs/epics/04-llm-proxy-and-governance.md § Story 4.3, 4.4
 */

import { query } from '../lib/database.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('MeteringService');

const DEFAULT_HOURLY_QUOTA = parseInt(process.env.DEFAULT_HOURLY_QUOTA ?? '100000', 10);
const DEFAULT_DAILY_QUOTA = parseInt(process.env.DEFAULT_DAILY_QUOTA ?? '1000000', 10);

export interface UsageRecord {
  agentId: string;
  circleId: string | null;
  model: string;
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  /** Estimated cost in USD — may be 0 when provider doesn't return pricing. */
  costUsd?: number;
  /** End-to-end proxy latency in milliseconds. */
  latencyMs?: number;
  /** 'success' for completed calls, 'error' for upstream failures. */
  status?: 'success' | 'error';
}

export interface BudgetStatus {
  allowed: boolean;
  hourlyUsed: number;
  hourlyQuota: number;
  dailyUsed: number;
  dailyQuota: number;
}

export class MeteringService {
  /**
   * Record a single LLM usage event.
   *
   * Writes to both:
   *   - token_usage: lightweight table used for fast budget window queries
   *   - usage_events: full audit table with cost, latency, and status
   */
  async recordUsage(record: UsageRecord): Promise<void> {
    const status = record.status ?? 'success';

    await Promise.all([
      // Budget query table (token_usage)
      query(
        `INSERT INTO token_usage
           (agent_id, circle_id, model, prompt_tokens, completion_tokens, total_tokens)
         VALUES ($1, $2, $3, $4, $5, $6)`,
        [
          record.agentId,
          record.circleId,
          record.model,
          record.promptTokens,
          record.completionTokens,
          record.totalTokens,
        ],
      ),
      // Full audit table (usage_events)
      query(
        `INSERT INTO usage_events
           (agent_id, model, prompt_tokens, completion_tokens, total_tokens,
            cost_usd, latency_ms, status)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)`,
        [
          record.agentId,
          record.model,
          record.promptTokens,
          record.completionTokens,
          record.totalTokens,
          record.costUsd ?? null,
          record.latencyMs ?? null,
          status,
        ],
      ),
    ]);

    logger.info(
      `Usage recorded | agent=${record.agentId} model=${record.model} ` +
      `prompt=${record.promptTokens} completion=${record.completionTokens} ` +
      `cost=${record.costUsd?.toFixed(6) ?? 'n/a'} latency=${record.latencyMs ?? 'n/a'}ms`,
    );
  }

  /**
   * Get total token usage for an agent within a time window.
   */
  async getUsage(agentId: string, windowHours: number): Promise<number> {
    const result = await query(
      `SELECT COALESCE(SUM(total_tokens), 0) AS total
       FROM token_usage
       WHERE agent_id = $1 AND created_at > NOW() - INTERVAL '1 hour' * $2`,
      [agentId, windowHours],
    );
    return parseInt(result.rows[0]?.total ?? '0', 10);
  }

  /**
   * Check whether an agent is within its budget.
   * Returns the budget status with used/quota for both hourly and daily windows.
   *
   * Story 4.3: This check must complete before any upstream LLM call is made.
   */
  async checkBudget(agentId: string): Promise<BudgetStatus> {
    // Fetch quota (or use defaults)
    const quotaResult = await query(
      `SELECT max_tokens_per_hour, max_tokens_per_day FROM token_quotas WHERE agent_id = $1`,
      [agentId],
    );

    const hourlyQuota = parseInt(
      quotaResult.rows[0]?.max_tokens_per_hour ?? String(DEFAULT_HOURLY_QUOTA),
      10,
    );
    const dailyQuota = parseInt(
      quotaResult.rows[0]?.max_tokens_per_day ?? String(DEFAULT_DAILY_QUOTA),
      10,
    );

    // Fetch current usage
    const hourlyUsed = await this.getUsage(agentId, 1);
    const dailyUsed = await this.getUsage(agentId, 24);

    const allowed = hourlyUsed < hourlyQuota && dailyUsed < dailyQuota;

    return { allowed, hourlyUsed, hourlyQuota, dailyUsed, dailyQuota };
  }

  /**
   * Get aggregated usage for the metering API.
   *
   * @param agentId  Filter by agent (optional)
   * @param from     Start of range (ISO8601)
   * @param to       End of range (ISO8601, defaults to now)
   * @param groupBy  'hour' | 'day'
   */
  async getAggregatedUsage(params: {
    agentId?: string;
    from?: string;
    to?: string;
    groupBy?: 'hour' | 'day';
  }): Promise<{ period: string; agentId?: string; totalTokens: number; costUsd: number }[]> {
    const groupBy = params.groupBy ?? 'day';
    const truncFn = groupBy === 'hour' ? 'hour' : 'day';

    const args: unknown[] = [];
    const conditions: string[] = [];

    if (params.agentId) {
      args.push(params.agentId);
      conditions.push(`agent_id = $${args.length}`);
    }
    if (params.from) {
      args.push(params.from);
      conditions.push(`created_at >= $${args.length}::timestamptz`);
    }
    if (params.to) {
      args.push(params.to);
      conditions.push(`created_at <= $${args.length}::timestamptz`);
    }

    const where = conditions.length > 0 ? `WHERE ${conditions.join(' AND ')}` : '';
    const agentIdCol = params.agentId ? '' : ', agent_id';

    const result = await query(
      `SELECT
         DATE_TRUNC('${truncFn}', created_at) AS period,
         COALESCE(SUM(total_tokens), 0) AS total_tokens,
         COALESCE(SUM(cost_usd), 0) AS cost_usd
         ${agentIdCol}
       FROM usage_events
       ${where}
       GROUP BY DATE_TRUNC('${truncFn}', created_at)${params.agentId ? '' : ', agent_id'}
       ORDER BY period ASC`,
      args,
    );

    return result.rows.map(row => ({
      period: (row.period as Date).toISOString(),
      ...(params.agentId ? {} : { agentId: row.agent_id as string }),
      totalTokens: parseInt(row.total_tokens, 10),
      costUsd: parseFloat(row.cost_usd) || 0,
    }));
  }

  /**
   * Get a summary of total usage for the current day across all agents.
   */
  async getDailySummary(): Promise<{
    date: string;
    totalTokens: number;
    totalCostUsd: number;
    agentCount: number;
  }> {
    const result = await query(
      `SELECT
         COALESCE(SUM(total_tokens), 0) AS total_tokens,
         COALESCE(SUM(cost_usd), 0) AS cost_usd,
         COUNT(DISTINCT agent_id) AS agent_count
       FROM usage_events
       WHERE created_at >= DATE_TRUNC('day', NOW())`,
    );

    const row = result.rows[0];
    return {
      date: new Date().toISOString().split('T')[0]!,
      totalTokens: parseInt(row?.total_tokens ?? '0', 10),
      totalCostUsd: parseFloat(row?.cost_usd ?? '0') || 0,
      agentCount: parseInt(row?.agent_count ?? '0', 10),
    };
  }
}
