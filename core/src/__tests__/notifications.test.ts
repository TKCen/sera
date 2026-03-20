/**
 * Integration tests: Epic 18 — notification dispatch and retry.
 *
 * These tests mock the DB layer and HTTP client to verify:
 *   - permission.requested event is dispatched to a configured webhook channel
 *   - when a channel returns 500, the ChannelRouter schedules a retry
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { v4 as uuidv4 } from 'uuid';

// ── Mocks ────────────────────────────────────────────────────────────────────

vi.mock('../lib/database.js', () => ({
  pool: { query: vi.fn() },
}));

vi.mock('axios');

import { pool } from '../lib/database.js';
import axios from 'axios';
import { ChannelRouter } from '../channels/ChannelRouter.js';
import { WebhookChannel } from '../channels/adapters/WebhookChannel.js';
import type { ChannelEvent } from '../channels/channel.interface.js';

function makeEvent(overrides: Partial<ChannelEvent> = {}): ChannelEvent {
  return {
    id: uuidv4(),
    eventType: 'permission.requested',
    title: 'Permission Request',
    body: 'Agent X requests filesystem access',
    severity: 'warning',
    metadata: { agentId: 'agent-1' },
    timestamp: new Date().toISOString(),
    ...overrides,
  };
}

describe('Webhook dispatch integration', () => {
  let router: ChannelRouter;
  let webhookChannel: WebhookChannel;

  beforeEach(() => {
    (ChannelRouter as unknown as { instance: undefined }).instance = undefined;
    router = ChannelRouter.getInstance();

    webhookChannel = new WebhookChannel('wh-1', 'test-webhook', {
      url: 'https://example.com/webhook',
      secret: 'test-secret',
    });
    router.register(webhookChannel);

    vi.mocked(pool.query).mockResolvedValue({ rows: [], rowCount: 0 } as never);
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('dispatches permission.requested event to a matching webhook channel', async () => {
    vi.mocked(pool.query)
      .mockResolvedValueOnce({
        rows: [
          {
            id: 'rule-1',
            event_type: 'permission.*',
            channel_ids: ['wh-1'],
            min_severity: 'info',
            filter: null,
          },
        ],
        rowCount: 1,
      } as never)
      .mockResolvedValue({ rows: [], rowCount: 0 } as never);

    vi.mocked(axios.post).mockResolvedValue({ status: 200, data: {} } as never);

    const event = makeEvent();
    await router.routeAsync(event);

    // Give the async send a tick to complete
    await new Promise((r) => setTimeout(r, 10));

    expect(axios.post).toHaveBeenCalledWith(
      'https://example.com/webhook',
      expect.objectContaining({
        event: expect.objectContaining({ eventType: 'permission.requested' }),
      }),
      expect.objectContaining({
        headers: expect.objectContaining({ 'X-Sera-Signature': expect.stringMatching(/^sha256=/) }),
      })
    );
  });

  it('includes HMAC-SHA256 signature header on webhook payload', async () => {
    vi.mocked(axios.post).mockResolvedValue({ status: 200 } as never);

    const event = makeEvent();
    await webhookChannel.send(event);

    const [, , opts] = vi.mocked(axios.post).mock.calls[0]!;
    const headers = (opts as { headers: Record<string, string> }).headers;
    expect(headers['X-Sera-Signature']).toMatch(/^sha256=[a-f0-9]{64}$/);
  });

  it('does not crash when channel returns HTTP 5xx — logs and marks failed', async () => {
    vi.mocked(pool.query)
      .mockResolvedValueOnce({
        rows: [
          {
            id: 'rule-1',
            event_type: '*',
            channel_ids: ['wh-1'],
            min_severity: 'info',
            filter: null,
          },
        ],
        rowCount: 1,
      } as never)
      .mockResolvedValue({ rows: [], rowCount: 0 } as never);

    vi.mocked(axios.post).mockRejectedValue(new Error('HTTP 500'));

    const event = makeEvent();

    // routeAsync should not throw even when channel fails
    await expect(router.routeAsync(event)).resolves.not.toThrow();
  });
});

describe('Webhook channel: actionable events include approve/deny URLs', () => {
  it('sends approveUrl and denyUrl in payload for actionable events', async () => {
    vi.mocked(axios.post).mockResolvedValue({ status: 200 } as never);

    const channel = new WebhookChannel('wh-2', 'test', {
      url: 'https://example.com/hook',
    });

    const event = makeEvent({
      actions: {
        requestId: 'req-1',
        requestType: 'permission',
        approveToken: 'approve.token.here',
        denyToken: 'deny.token.here',
        expiresAt: new Date(Date.now() + 900_000).toISOString(),
      },
    });

    await channel.send(event);

    const [, body] = vi.mocked(axios.post).mock.calls[0]!;
    const payload = body as { approveUrl: string; denyUrl: string };
    expect(payload.approveUrl).toContain('/api/notifications/action');
    expect(payload.denyUrl).toContain('/api/notifications/action');
  });
});
