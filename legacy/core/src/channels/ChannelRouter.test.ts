import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { matchesPattern, ChannelRouter } from './ChannelRouter.js';
import type { Channel, ChannelEvent, ChannelHealth } from './channel.interface.js';

// ── Wildcard matching unit tests ─────────────────────────────────────────────

describe('matchesPattern()', () => {
  it('* matches any event type', () => {
    expect(matchesPattern('*', 'permission.requested')).toBe(true);
    expect(matchesPattern('*', 'agent.crashed')).toBe(true);
    expect(matchesPattern('*', 'x')).toBe(true);
  });

  it('exact pattern matches identical event type', () => {
    expect(matchesPattern('permission.requested', 'permission.requested')).toBe(true);
    expect(matchesPattern('permission.requested', 'permission.denied')).toBe(false);
  });

  it('wildcard suffix matches sub-types', () => {
    expect(matchesPattern('permission.*', 'permission.requested')).toBe(true);
    expect(matchesPattern('permission.*', 'permission.granted')).toBe(true);
    expect(matchesPattern('permission.*', 'permission.denied')).toBe(true);
    expect(matchesPattern('permission.*', 'permission')).toBe(true);
  });

  it('wildcard suffix does not match unrelated prefix', () => {
    expect(matchesPattern('permission.*', 'agent.crashed')).toBe(false);
    expect(matchesPattern('permission.*', 'permissionx.requested')).toBe(false);
  });

  it('system.* matches system events only', () => {
    expect(matchesPattern('system.*', 'system.test')).toBe(true);
    expect(matchesPattern('system.*', 'system.health')).toBe(true);
    expect(matchesPattern('system.*', 'agent.crashed')).toBe(false);
  });
});

// ── ChannelRouter dispatch tests ─────────────────────────────────────────────

vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn(),
  },
}));

import { pool } from '../lib/database.js';

function makeChannel(id: string): Channel & { sent: ChannelEvent[] } {
  const sent: ChannelEvent[] = [];
  return {
    id,
    type: 'webhook',
    name: `channel-${id}`,
    sent,
    async send(event: ChannelEvent) {
      sent.push(event);
    },
    async healthCheck(): Promise<ChannelHealth> {
      return { healthy: true };
    },
  };
}

function makeEvent(eventType: string, severity: ChannelEvent['severity'] = 'info'): ChannelEvent {
  return {
    id: 'evt-1',
    eventType,
    title: 'Test',
    body: 'body',
    severity,
    metadata: {},
    timestamp: new Date().toISOString(),
  };
}

describe('ChannelRouter', () => {
  let router: ChannelRouter;

  beforeEach(() => {
    (ChannelRouter as unknown as { instance: undefined }).instance = undefined;
    router = ChannelRouter.getInstance();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('dispatches to a channel matching exact event type', async () => {
    const ch = makeChannel('c1');
    router.register(ch);

    vi.mocked(pool.query)
      .mockResolvedValueOnce({
        rows: [
          {
            id: 'rule-1',
            event_type: 'permission.requested',
            channel_ids: ['c1'],
            min_severity: 'info',
            filter: null,
            enabled: true,
            priority: 0,
            target_agent_id: null,
          },
        ],
        rowCount: 1,
      } as never)
      .mockResolvedValue({ rows: [], rowCount: 0 } as never);

    await router.routeAsync(makeEvent('permission.requested'));
    expect(ch.sent).toHaveLength(1);
  });

  it('dispatches to a channel matching wildcard rule', async () => {
    const ch = makeChannel('c2');
    router.register(ch);

    vi.mocked(pool.query)
      .mockResolvedValueOnce({
        rows: [
          {
            id: 'rule-2',
            event_type: 'permission.*',
            channel_ids: ['c2'],
            min_severity: 'info',
            filter: null,
            enabled: true,
            priority: 0,
            target_agent_id: null,
          },
        ],
        rowCount: 1,
      } as never)
      .mockResolvedValue({ rows: [], rowCount: 0 } as never);

    await router.routeAsync(makeEvent('permission.denied'));
    expect(ch.sent).toHaveLength(1);
  });

  it('does not dispatch when severity is below minSeverity', async () => {
    const ch = makeChannel('c3');
    router.register(ch);

    vi.mocked(pool.query)
      .mockResolvedValueOnce({
        rows: [
          {
            id: 'rule-3',
            event_type: '*',
            channel_ids: ['c3'],
            min_severity: 'critical',
            filter: null,
            enabled: true,
            priority: 0,
            target_agent_id: null,
          },
        ],
        rowCount: 1,
      } as never)
      .mockResolvedValue({ rows: [], rowCount: 0 } as never);

    await router.routeAsync(makeEvent('something', 'warning'));
    expect(ch.sent).toHaveLength(0);
  });

  it('does not dispatch when filter does not match metadata', async () => {
    const ch = makeChannel('c4');
    router.register(ch);

    vi.mocked(pool.query)
      .mockResolvedValueOnce({
        rows: [
          {
            id: 'rule-4',
            event_type: '*',
            channel_ids: ['c4'],
            min_severity: 'info',
            filter: { agentId: 'agent-x' },
            enabled: true,
            priority: 0,
            target_agent_id: null,
          },
        ],
        rowCount: 1,
      } as never)
      .mockResolvedValue({ rows: [], rowCount: 0 } as never);

    const event = makeEvent('something');
    event.metadata = { agentId: 'agent-y' };

    await router.routeAsync(event);
    expect(ch.sent).toHaveLength(0);
  });

  it('does not throw when the channel send fails', async () => {
    const ch = makeChannel('c5');
    ch.send = async () => {
      throw new Error('network error');
    };
    router.register(ch);

    vi.mocked(pool.query)
      .mockResolvedValueOnce({
        rows: [
          {
            id: 'rule-5',
            event_type: '*',
            channel_ids: ['c5'],
            min_severity: 'info',
            filter: null,
            enabled: true,
            priority: 0,
            target_agent_id: null,
          },
        ],
        rowCount: 1,
      } as never)
      .mockResolvedValue({ rows: [], rowCount: 0 } as never);

    await expect(router.routeAsync(makeEvent('something'))).resolves.not.toThrow();
  });

  it('respects target_agent_id in rule', async () => {
    const chX = makeChannel('cX');
    const chY = makeChannel('cY');
    router.register(chX);
    router.register(chY);

    vi.mocked(pool.query).mockResolvedValue({
      rows: [
        {
          id: 'rule-X',
          event_type: '*',
          channel_ids: ['cX'],
          min_severity: 'info',
          filter: null,
          enabled: true,
          priority: 0,
          target_agent_id: 'agent-X',
        },
        {
          id: 'rule-Y',
          event_type: '*',
          channel_ids: ['cY'],
          min_severity: 'info',
          filter: null,
          enabled: true,
          priority: 0,
          target_agent_id: 'agent-Y',
        },
      ],
      rowCount: 2,
    } as never);

    const event = makeEvent('test');
    event.metadata = { agentId: 'agent-X' };

    await router.routeAsync(event);
    expect(chX.sent).toHaveLength(1);
    expect(chY.sent).toHaveLength(0);
  });

  it('respects enabled flag in rule', async () => {
    const ch = makeChannel('cEnabled');
    router.register(ch);

    vi.mocked(pool.query).mockResolvedValue({
      rows: [],
      rowCount: 0,
    } as never);

    await router.routeAsync(makeEvent('test'));
    expect(ch.sent).toHaveLength(0);
  });
});
