import { Logger } from '../lib/logger.js';

export interface IncomingMessage {
  platform: string;
  userId: string;
  userName: string;
  chatId: string;
  text: string;
  metadata?: Record<string, unknown>;
}

export abstract class ChannelAdapter {
  protected logger: Logger;
  protected userRateLimits: Map<string, { count: number; lastReset: number }> = new Map();
  protected rateLimitWindow: number;
  protected maxMessagesPerWindow: number;
  private cleanupInterval: NodeJS.Timeout | null = null;

  constructor(
    public readonly platform: string,
    options?: { rateLimitWindow?: number; maxMessagesPerWindow?: number }
  ) {
    this.logger = new Logger(`Channel:${platform}`);
    this.rateLimitWindow = options?.rateLimitWindow || 60 * 1000;
    this.maxMessagesPerWindow = options?.maxMessagesPerWindow || 20;

    // Periodic cleanup to avoid memory leak
    this.cleanupInterval = setInterval(() => this.cleanupRateLimits(), this.rateLimitWindow * 2);
  }

  /**
   * Check if a user is rate limited.
   */
  protected isRateLimited(userId: string): boolean {
    const now = Date.now();
    const limit = this.userRateLimits.get(userId);

    if (!limit || now - limit.lastReset > this.rateLimitWindow) {
      this.userRateLimits.set(userId, { count: 1, lastReset: now });
      return false;
    }

    if (limit.count >= this.maxMessagesPerWindow) {
      return true;
    }

    limit.count++;
    return false;
  }

  /**
   * Remove stale rate limit entries to prevent OOM.
   */
  private cleanupRateLimits() {
    const now = Date.now();
    for (const [userId, limit] of this.userRateLimits.entries()) {
      if (now - limit.lastReset > this.rateLimitWindow * 2) {
        this.userRateLimits.delete(userId);
      }
    }
  }

  abstract start(): Promise<void>;
  abstract stop(): Promise<void>;
  abstract sendMessage(chatId: string, text: string): Promise<void>;

  protected async shutdownBase() {
    if (this.cleanupInterval) {
      clearInterval(this.cleanupInterval);
      this.cleanupInterval = null;
    }
  }
}
