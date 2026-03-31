import { describe, it, expect } from 'vitest';
import { ChannelAdapter } from './ChannelAdapter.js';

// Concrete test subclass to access protected methods
class TestAdapter extends ChannelAdapter {
  public sentMessages: Array<{ chatId: string; text: string }> = [];

  constructor(options?: { messageTimeoutMs?: number }) {
    super('Test', options);
  }

  async start() {}
  async stop() {
    await this.shutdownBase();
  }

  async sendMessage(chatId: string, text: string) {
    this.sentMessages.push({ chatId, text });
  }

  // Expose protected method for testing
  public testEnqueue(key: string, task: () => Promise<void>) {
    this.enqueueMessage(key, task);
  }

  public testIsRateLimited(userId: string) {
    return this.isRateLimited(userId);
  }
}

describe('ChannelAdapter', () => {
  describe('message queue', () => {
    it('should process messages sequentially for the same key', async () => {
      const adapter = new TestAdapter();
      const order: number[] = [];

      const task1Done = new Promise<void>((resolve) => {
        adapter.testEnqueue('user:1', async () => {
          order.push(1);
          // Simulate slow processing
          await new Promise((r) => setTimeout(r, 50));
          order.push(2);
          resolve();
        });
      });

      const task2Done = new Promise<void>((resolve) => {
        adapter.testEnqueue('user:1', async () => {
          order.push(3);
          resolve();
        });
      });

      await Promise.all([task1Done, task2Done]);

      // Task 2 should only start after task 1 finishes
      expect(order).toEqual([1, 2, 3]);
    });

    it('should process messages in parallel for different keys', async () => {
      const adapter = new TestAdapter();
      const order: string[] = [];

      const task1Done = new Promise<void>((resolve) => {
        adapter.testEnqueue('user:A', async () => {
          order.push('A-start');
          await new Promise((r) => setTimeout(r, 50));
          order.push('A-end');
          resolve();
        });
      });

      const task2Done = new Promise<void>((resolve) => {
        adapter.testEnqueue('user:B', async () => {
          order.push('B-start');
          await new Promise((r) => setTimeout(r, 10));
          order.push('B-end');
          resolve();
        });
      });

      await Promise.all([task1Done, task2Done]);

      // B should start before A ends (parallel for different keys)
      const aEndIdx = order.indexOf('A-end');
      const bStartIdx = order.indexOf('B-start');
      expect(bStartIdx).toBeLessThan(aEndIdx);
    });

    it('should handle task errors without blocking the queue', async () => {
      const adapter = new TestAdapter();
      const results: string[] = [];

      adapter.testEnqueue('user:1', async () => {
        throw new Error('Task 1 failed');
      });

      const task2Done = new Promise<void>((resolve) => {
        adapter.testEnqueue('user:1', async () => {
          results.push('task2-ok');
          resolve();
        });
      });

      await task2Done;
      expect(results).toEqual(['task2-ok']);
    });

    it('should timeout tasks that run too long', async () => {
      const adapter = new TestAdapter({ messageTimeoutMs: 50 });
      const results: string[] = [];

      adapter.testEnqueue('user:1', async () => {
        // This task takes too long and should be timed out
        await new Promise((r) => setTimeout(r, 200));
        results.push('slow-task');
      });

      const task2Done = new Promise<void>((resolve) => {
        adapter.testEnqueue('user:1', async () => {
          results.push('fast-task');
          resolve();
        });
      });

      await task2Done;

      // The fast task should complete, slow task was timed out
      expect(results).toContain('fast-task');
    });
  });

  describe('rate limiting', () => {
    it('should rate limit after maxMessagesPerWindow', () => {
      const adapter = new TestAdapter();
      // Default is 20 messages per window
      for (let i = 0; i < 20; i++) {
        expect(adapter.testIsRateLimited('user1')).toBe(false);
      }
      expect(adapter.testIsRateLimited('user1')).toBe(true);
    });

    it('should not rate limit different users', () => {
      const adapter = new TestAdapter();
      for (let i = 0; i < 20; i++) {
        adapter.testIsRateLimited('user1');
      }
      expect(adapter.testIsRateLimited('user1')).toBe(true);
      expect(adapter.testIsRateLimited('user2')).toBe(false);
    });
  });
});
