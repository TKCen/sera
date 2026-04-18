import { Logger } from '../lib/logger.js';

export interface IncomingMessage {
  platform: string;
  userId: string;
  userName: string;
  chatId: string;
  text: string;
  metadata?: Record<string, unknown>;
}

/** Default per-message processing timeout (2 minutes). */
const DEFAULT_MESSAGE_TIMEOUT_MS = 120_000;

export abstract class ChannelAdapter {
  protected logger: Logger;
  protected userRateLimits: Map<string, { count: number; lastReset: number }> = new Map();
  protected rateLimitWindow: number;
  protected maxMessagesPerWindow: number;
  private cleanupInterval: NodeJS.Timeout | null = null;

  /**
   * Per-key FIFO message queue.
   * Each key (e.g. `{channelId}:{userId}`) has an array of pending tasks
   * and a flag indicating whether the queue is currently draining.
   */
  private messageQueues: Map<string, { tasks: Array<() => Promise<void>>; draining: boolean }> =
    new Map();
  protected messageTimeoutMs: number;

  constructor(
    public readonly platform: string,
    options?: {
      rateLimitWindow?: number;
      maxMessagesPerWindow?: number;
      messageTimeoutMs?: number;
    }
  ) {
    this.logger = new Logger(`Channel:${platform}`);
    this.rateLimitWindow = options?.rateLimitWindow || 60 * 1000;
    this.maxMessagesPerWindow = options?.maxMessagesPerWindow || 20;
    this.messageTimeoutMs = options?.messageTimeoutMs || DEFAULT_MESSAGE_TIMEOUT_MS;

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

  /**
   * Enqueue a message-processing task for sequential execution.
   * Messages with the same key are processed one at a time in FIFO order.
   * @param key Unique queue key (e.g. `{channelId}:{userId}`)
   * @param task Async function that processes the message
   */
  protected enqueueMessage(key: string, task: () => Promise<void>): void {
    let queue = this.messageQueues.get(key);
    if (!queue) {
      queue = { tasks: [], draining: false };
      this.messageQueues.set(key, queue);
    }
    queue.tasks.push(task);

    if (!queue.draining) {
      this.drainQueue(key);
    }
  }

  private drainQueue(key: string): void {
    const queue = this.messageQueues.get(key);
    if (!queue || queue.tasks.length === 0) {
      if (queue) {
        queue.draining = false;
      }
      return;
    }

    queue.draining = true;
    const task = queue.tasks.shift()!;

    const timeoutPromise = new Promise<void>((_, reject) =>
      setTimeout(() => reject(new Error('Message processing timeout')), this.messageTimeoutMs)
    );

    Promise.race([task(), timeoutPromise])
      .catch((err) => {
        this.logger.error(`Message queue error for key "${key}":`, (err as Error).message);
      })
      .finally(() => {
        this.drainQueue(key);
      });
  }

  abstract start(): Promise<void>;
  abstract stop(): Promise<void>;
  abstract sendMessage(chatId: string, text: string): Promise<void>;

  protected async shutdownBase() {
    if (this.cleanupInterval) {
      clearInterval(this.cleanupInterval);
      this.cleanupInterval = null;
    }
    this.messageQueues.clear();
  }
}
