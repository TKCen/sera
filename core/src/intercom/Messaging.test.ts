import { describe, it, expect, vi, beforeEach } from 'vitest';
import { IntercomService } from './IntercomService.js';
import { WebhooksService } from './WebhooksService.js';
import { ChannelNamespace } from './ChannelNamespace.js';
import crypto from 'crypto';

// ── Mocks ───────────────────────────────────────────────────────────────────────

vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn().mockResolvedValue({ rows: [] }),
  },
}));

vi.mock('axios', () => ({
  default: {
    create: vi.fn(() => ({
      post: vi.fn().mockResolvedValue({ data: { result: {} } }),
    })),
  },
}));

// ── Tests ───────────────────────────────────────────────────────────────────────

describe('Epic 09: Real-Time Messaging', () => {
  let intercom: IntercomService;
  let webhooks: WebhooksService;

  beforeEach(() => {
    vi.clearAllMocks();
    intercom = new IntercomService();
    webhooks = new WebhooksService(intercom);
  });

  describe('Story 9.1: Channel Namespace', () => {
    it('generates canonical channel names', () => {
      expect(ChannelNamespace.thoughts('agent-1')).toBe('thoughts:agent-1');
      expect(ChannelNamespace.tokens('agent-1')).toBe('tokens:agent-1');
      expect(ChannelNamespace.status('agent-1')).toBe('agent:agent-1:status');
      expect(ChannelNamespace.private('agent-1', 'agent-2')).toBe('private:agent-1:agent-2');
      expect(ChannelNamespace.circle('circle-1')).toBe('circle:circle-1');
      expect(ChannelNamespace.system('event')).toBe('system.event');
    });

    it('validates canonical channel names', () => {
      expect(ChannelNamespace.isValid('thoughts:agent-1')).toBe(true);
      expect(ChannelNamespace.isValid('tokens:agent-1')).toBe(true);
      expect(ChannelNamespace.isValid('agent:agent-1:status')).toBe(true);
      expect(ChannelNamespace.isValid('private:agent-1:agent-2')).toBe(true);
      expect(ChannelNamespace.isValid('circle:circle-1')).toBe(true);
      expect(ChannelNamespace.isValid('system.event')).toBe(true);

      expect(ChannelNamespace.isValid('invalid:channel')).toBe(false);
      expect(ChannelNamespace.isValid('system:event')).toBe(false);
      expect(ChannelNamespace.isValid('thoughts:agent:extra')).toBe(false);
    });
  });

  describe('Story 9.5: Subscription Tokens', () => {
    it('generates a valid connection token', async () => {
      const token = await intercom.generateConnectionToken('user-1');
      expect(token).toBeDefined();
      expect(typeof token).toBe('string');
    });

    it('allows admin/operator to subscribe to any channel', async () => {
      const adminToken = await intercom.generateSubscriptionToken('user-1', 'private:a:b', 'admin');
      expect(adminToken).toBeDefined();

      const opToken = await intercom.generateSubscriptionToken('user-1', 'tokens:a', 'operator');
      expect(opToken).toBeDefined();
    });

    it('restricts viewer to thought streams only', async () => {
      // Allowed
      const thoughtToken = await intercom.generateSubscriptionToken('user-1', 'thoughts:a', 'viewer');
      expect(thoughtToken).toBeDefined();

      // Denied
      await expect(intercom.generateSubscriptionToken('user-1', 'tokens:a', 'viewer'))
        .rejects.toThrow('Role "viewer" is only permitted to subscribe to thought streams');
      
      await expect(intercom.generateSubscriptionToken('user-1', 'private:a:b', 'viewer'))
        .rejects.toThrow('Role "viewer" is only permitted to subscribe to thought streams');
    });
  });

  describe('Story 9.8: Webhooks', () => {
    const secret = 'test-secret';
    const body = JSON.stringify({ event: 'test' });
    const timestamp = Date.now().toString();

    const computeSignature = (s: string, b: string, t: string) => {
      return crypto.createHmac('sha256', s).update(`${t}.${b}`).digest('hex');
    };

    it('validates a correct signature', () => {
      const signature = computeSignature(secret, body, timestamp);
      const isValid = webhooks.verifySignature(secret, body, signature, timestamp);
      expect(isValid).toBe(true);
    });

    it('rejects an invalid signature', () => {
      const isValid = webhooks.verifySignature(secret, body, 'invalid-sig', timestamp);
      expect(isValid).toBe(false);
    });

    it('rejects an expired timestamp', () => {
      const oldTimestamp = (Date.now() - 10 * 60 * 1000).toString(); // 10 minutes ago
      const signature = computeSignature(secret, body, oldTimestamp);
      const isValid = webhooks.verifySignature(secret, body, signature, oldTimestamp);
      expect(isValid).toBe(false);
    });

    it('rejects a replay (duplicate nonce)', () => {
      const signature = computeSignature(secret, body, timestamp);
      const nonce = 'nonce-123';
      
      const firstTry = webhooks.verifySignature(secret, body, signature, timestamp, nonce);
      expect(firstTry).toBe(true);

      const secondTry = webhooks.verifySignature(secret, body, signature, timestamp, nonce);
      expect(secondTry).toBe(false);
    });
  });
});
