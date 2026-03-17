/**
 * MeteringService — tracks token usage and enforces budget quotas.
 *
 * Records per-request LLM token usage to the `token_usage` table and
 * checks agents against their quota in `token_quotas`.
 *
 * @see docs/v2-distributed-architecture/02-security-and-gateway.md § Token Metering
 */

import { query } from '../lib/database.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('MeteringService');

const DEFAULT_HOURLY_QUOTA = parseInt(process.env.DEFAULT_HOURLY_QUOTA || '100000', 10);
const DEFAULT_DAILY_QUOTA = parseInt(process.env.DEFAULT_DAILY_QUOTA || '1000000', 10);

export interface UsageRecord {
  agentId: string;
  circleId: string | null;
  model: string;
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
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
   */
  async recordUsage(record: UsageRecord): Promise<void> {
    await query(
      `INSERT INTO token_usage (agent_id, circle_id, model, prompt_tokens, completion_tokens, total_tokens)
       VALUES ($1, $2, $3, $4, $5, $6)`,
      [
        record.agentId,
        record.circleId,
        record.model,
        record.promptTokens,
        record.completionTokens,
        record.totalTokens,
      ],
    );
    logger.info(
      `Usage recorded | agent=${record.agentId} model=${record.model} ` +
      `prompt=${record.promptTokens} completion=${record.completionTokens}`,
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
   */
  async checkBudget(agentId: string): Promise<BudgetStatus> {
    // Fetch quota (or use defaults)
    const quotaResult = await query(
      `SELECT max_tokens_per_hour, max_tokens_per_day FROM token_quotas WHERE agent_id = $1`,
      [agentId],
    );

    const hourlyQuota = parseInt(quotaResult.rows[0]?.max_tokens_per_hour ?? String(DEFAULT_HOURLY_QUOTA), 10);
    const dailyQuota = parseInt(quotaResult.rows[0]?.max_tokens_per_day ?? String(DEFAULT_DAILY_QUOTA), 10);

    // Fetch current usage
    const hourlyUsed = await this.getUsage(agentId, 1);
    const dailyUsed = await this.getUsage(agentId, 24);

    const allowed = hourlyUsed < hourlyQuota && dailyUsed < dailyQuota;

    return { allowed, hourlyUsed, hourlyQuota, dailyUsed, dailyQuota };
  }
}
