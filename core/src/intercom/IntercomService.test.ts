import { describe, it, expect, vi, beforeEach } from 'vitest';
import { IntercomService, IntercomPermissionError } from './IntercomService.js';
import type { AgentManifest } from '../agents/manifest/types.js';

// ── Mock axios ──────────────────────────────────────────────────────────────────
vi.mock('axios', () => {
  const mockPost = vi.fn().mockResolvedValue({ data: { result: {} } });
  return {
    default: {
      create: vi.fn(() => ({
        post: mockPost,
      })),
    },
    AxiosError: class AxiosError extends Error {
      constructor(message: string) {
        super(message);
        this.name = 'AxiosError';
      }
    },
  };
});

// ── Helpers ─────────────────────────────────────────────────────────────────────

function createManifest(overrides: Partial<AgentManifest> = {}): AgentManifest {
  return {
    apiVersion: 'sera/v1',
    kind: 'Agent',
    metadata: {
      name: 'architect-prime',
      displayName: 'Winston',
      icon: '🏗️',
      circle: 'development',
      tier: 2,
    },
    identity: {
      role: 'System Architect',
      description: 'A test agent',
    },
    model: {
      provider: 'lm-studio',
      name: 'test-model',
    },
    intercom: {
      canMessage: ['developer-prime', 'reviewer-prime'],
      channels: {
        publish: ['architecture-decisions'],
        subscribe: ['code-review-requests'],
      },
    },
    ...overrides,
  };
}

// ── Tests ───────────────────────────────────────────────────────────────────────

describe('IntercomService', () => {
  let service: IntercomService;

  beforeEach(() => {
    vi.clearAllMocks();
    service = new IntercomService('http://test:8000/api', 'test-key');
  });

  describe('sendDirectMessage', () => {
    it('sends a message to a permitted peer', async () => {
      const manifest = createManifest();
      const msg = await service.sendDirectMessage(manifest, 'developer-prime', {
        text: 'Hello!',
      });

      expect(msg).toBeDefined();
      expect(msg.source.agent).toBe('architect-prime');
      expect(msg.source.circle).toBe('development');
      expect(msg.type).toBe('message');
      expect(msg.payload).toEqual({ text: 'Hello!' });
      // DM channel should be sorted
      expect(msg.target.channel).toBe('intercom:development:architect-prime:developer-prime');
    });

    it('throws IntercomPermissionError for unpermitted peer', async () => {
      const manifest = createManifest();
      await expect(
        service.sendDirectMessage(manifest, 'unknown-agent', { text: 'Hi' }),
      ).rejects.toThrow(IntercomPermissionError);
    });

    it('throws IntercomPermissionError when agent has no intercom config', async () => {
      const manifest = createManifest();
      delete (manifest as any).intercom;
      await expect(
        service.sendDirectMessage(manifest, 'developer-prime', { text: 'Hi' }),
      ).rejects.toThrow(IntercomPermissionError);
    });
  });

  describe('publishThought', () => {
    it('publishes to the correct thoughts channel', async () => {
      // This should not throw
      await service.publishThought('architect-prime', 'Winston', 'observe', 'Looking at code...');
    });
  });

  describe('publishToCircleChannel', () => {
    it('publishes to a permitted channel', async () => {
      const manifest = createManifest();
      const msg = await service.publishToCircleChannel(manifest, 'architecture-decisions', {
        decision: 'Use REST over gRPC',
      });

      expect(msg.target.channel).toBe('channel:development:architecture-decisions');
      expect(msg.type).toBe('message');
    });

    it('throws when agent is not permitted to publish', async () => {
      const manifest = createManifest();
      await expect(
        service.publishToCircleChannel(manifest, 'unknown-channel', { data: 'test' }),
      ).rejects.toThrow('not permitted to publish');
    });
  });

  describe('getAgentChannels', () => {
    it('returns all channels for an agent', () => {
      const manifest = createManifest();
      const channels = service.getAgentChannels(manifest);

      expect(channels.thoughts).toBe('internal:agent:architect-prime:thoughts');
      expect(channels.terminal).toBe('internal:agent:architect-prime:terminal');
      expect(channels.publishChannels).toEqual([
        'channel:development:architecture-decisions',
      ]);
      expect(channels.subscribeChannels).toEqual([
        'channel:development:code-review-requests',
      ]);
      expect(channels.dmPeers).toHaveLength(2);
    });

    it('handles agent with no intercom config', () => {
      const manifest = createManifest();
      delete (manifest as any).intercom;
      const channels = service.getAgentChannels(manifest);

      expect(channels.thoughts).toBe('internal:agent:architect-prime:thoughts');
      expect(channels.publishChannels).toEqual([]);
      expect(channels.subscribeChannels).toEqual([]);
      expect(channels.dmPeers).toEqual([]);
    });
  });

  describe('getHistory', () => {
    it('returns an empty array on error', async () => {
      const result = await service.getHistory('test-channel');
      // Default mock returns { result: {} } with no publications
      expect(result).toEqual([]);
    });
  });
});
