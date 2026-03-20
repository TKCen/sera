import { query } from '../lib/database.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('MeteringEngine');

export interface UsageEvent {
  agentId: string;
  model: string;
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

export class MeteringEngine {
  /**
   * Record a single usage event to the database.
   */
  async record(event: UsageEvent): Promise<void> {
    try {
      await query(
        `INSERT INTO usage_events (agent_id, model, prompt_tokens, completion_tokens, total_tokens)
         VALUES ($1, $2, $3, $4, $5)`,
        [event.agentId, event.model, event.promptTokens, event.completionTokens, event.totalTokens]
      );
      logger.debug(
        `Recorded usage for agent ${event.agentId}: ${event.totalTokens} tokens (${event.model})`
      );
    } catch (err: any) {
      logger.error(`Failed to record usage event for agent ${event.agentId}:`, err);
    }
  }
}
