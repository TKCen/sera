import { query } from '../lib/database.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('AgentScheduler');

export class AgentScheduler {
  /**
   * Check if an agent is within its hourly token quota.
   * @param agentId The unique ID of the agent.
   * @param limit The hourly token limit for the agent.
   * @returns Promise<boolean> True if the agent is within quota.
   */
  async isWithinQuota(agentId: string, limit: number): Promise<boolean> {
    try {
      const result = await query(
        `SELECT COALESCE(SUM(total_tokens), 0) AS total
         FROM usage_events
         WHERE agent_id = $1 AND created_at > NOW() - INTERVAL '1 hour'`,
        [agentId]
      );

      const used = parseInt(result.rows[0]?.total ?? '0', 10);
      const isWithin = used < limit;

      if (!isWithin) {
        logger.warn(`Agent ${agentId} exceeded hourly quota: ${used} / ${limit}`);
      }

      return isWithin;
    } catch (err: any) {
      logger.error(`Error checking quota for agent ${agentId}:`, err);
      // Fail-open: if DB check fails, allow the request
      return true;
    }
  }
}
