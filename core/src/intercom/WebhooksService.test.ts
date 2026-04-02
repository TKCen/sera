import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import crypto from 'crypto';
import { WebhooksService } from './WebhooksService.js';
import type { IntercomService } from './IntercomService.js';
import { pool } from '../lib/database.js';

vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn(),
  },
}));

describe('WebhooksService', () => {
  let webhooksService: WebhooksService;
  let mockIntercom: vi.Mocked<IntercomService>;

  beforeEach(() => {
    vi.useFakeTimers();
    mockIntercom = {
      publishSystemEvent: vi.fn(),
    } as unknown as vi.Mocked<IntercomService>;
    webhooksService = new WebhooksService(mockIntercom);
  });

  afterEach(() => {
    vi.clearAllMocks();
    vi.useRealTimers();
    // Stop the interval to allow tests to exit
    // @ts-ignore
    clearInterval(webhooksService.nonceCleanupInterval);
  });

  describe('verifySignature', () => {
    const secret = 'test-secret';
    const body = '{"foo":"bar"}';
    const timestamp = Date.now().toString();

    it('should return true for valid signature', () => {
      const hmac = crypto.createHmac('sha256', secret);
      const signature = hmac.update(`${timestamp}.${body}`).digest('hex');

      const result = webhooksService.verifySignature(secret, body, signature, timestamp);
      expect(result).toBe(true);
    });

    it('should return false for invalid signature', () => {
      const result = webhooksService.verifySignature(secret, body, 'invalid-sig', timestamp);
      expect(result).toBe(false);
    });

    it('should return false for expired timestamp', () => {
      const hmac = crypto.createHmac('sha256', secret);
      const oldTimestamp = (Date.now() - 10 * 60 * 1000).toString();
      const signature = hmac.update(`${oldTimestamp}.${body}`).digest('hex');

      const result = webhooksService.verifySignature(secret, body, signature, oldTimestamp);
      expect(result).toBe(false);
    });

    it('should return false for duplicate nonce', () => {
      const hmac = crypto.createHmac('sha256', secret);
      const signature = hmac.update(`${timestamp}.${body}`).digest('hex');
      const nonce = 'once-only';

      const first = webhooksService.verifySignature(secret, body, signature, timestamp, nonce);
      expect(first).toBe(true);

      const second = webhooksService.verifySignature(secret, body, signature, timestamp, nonce);
      expect(second).toBe(false);
    });
  });

  describe('handleIncoming', () => {
    const secret = 'test-secret';
    const rawBody = '{"foo":"bar"}';
    const timestamp = Date.now().toString();
    const slug = 'test-hook';
    const hmac = crypto.createHmac('sha256', secret);
    const signature = hmac.update(`${timestamp}.${rawBody}`).digest('hex');

    it('should process a valid webhook and publish to intercom', async () => {
      vi.mocked(pool.query).mockResolvedValueOnce({
        rows: [{ id: 'hook-1', secret, enabled: true, event_type: 'test.event' }],
      } as any);

      vi.mocked(pool.query).mockResolvedValueOnce({
        rows: [{ id: 'delivery-1' }],
      } as any);

      await webhooksService.handleIncoming(slug, rawBody, signature, timestamp);

      expect(pool.query).toHaveBeenCalledWith(expect.stringContaining('SELECT * FROM webhooks'), [
        slug,
      ]);
      expect(pool.query).toHaveBeenCalledWith(
        expect.stringContaining('INSERT INTO webhook_deliveries'),
        ['hook-1', JSON.parse(rawBody), 'pending']
      );

      // Wait for async publishAndLog to complete its database updates
      await vi.waitFor(() => {
        if (vi.mocked(pool.query).mock.calls.some(call =>
          typeof call[0] === 'string' && call[0].includes('UPDATE webhook_deliveries SET status = $1')
        )) {
          return true;
        }
        throw new Error('Not updated yet');
      });

      expect(mockIntercom.publishSystemEvent).toHaveBeenCalledWith('test.event', {
        raw: rawBody,
        wrapped: expect.stringContaining(rawBody),
        data: JSON.parse(rawBody),
      });

      expect(pool.query).toHaveBeenCalledWith(
        expect.stringContaining('UPDATE webhook_deliveries SET status = $1'),
        ['success', expect.any(Date), 'delivery-1']
      );
    });

    it('should throw error if webhook not found', async () => {
      vi.mocked(pool.query).mockResolvedValueOnce({ rows: [] } as any);

      await expect(
        webhooksService.handleIncoming(slug, rawBody, signature, timestamp)
      ).rejects.toThrow('Webhook not found or disabled');
    });

    it('should throw error if signature invalid', async () => {
      vi.mocked(pool.query).mockResolvedValueOnce({
        rows: [{ id: 'hook-1', secret, enabled: true, event_type: 'test.event' }],
      } as any);

      await expect(
        webhooksService.handleIncoming(slug, rawBody, 'wrong-sig', timestamp)
      ).rejects.toThrow('Invalid webhook signature');
    });
  });
});
