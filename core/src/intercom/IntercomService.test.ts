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
  } as AgentManifest;
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
      // Private channel should be sorted architect-prime < developer-prime
      expect(msg.target.channel).toBe('private:architect-prime:developer-prime');
    });

    it('throws IntercomPermissionError for unpermitted peer', async () => {
      const manifest = createManifest();
      await expect(
        service.sendDirectMessage(manifest, 'unknown-agent', { text: 'Hi' }),
      ).rejects.toThrow(IntercomPermissionError);
    });
  });

  describe('publishThought', () => {
    it('publishes to the correct thoughts channel', async () => {
      await service.publishThought('architect-prime', 'Winston', 'observe', 'Looking at code...');
    });
  });

  describe('broadcastToCircle', () => {
    it('publishes to a permitted circle', async () => {
      const manifest = createManifest();
      const msg = await service.broadcastToCircle(manifest, 'development', {
        decision: 'Use REST over gRPC',
      });

      expect(msg.target.channel).toBe('circle:development');
      expect(msg.type).toBe('message');
    });

    it('throws when agent is not a member of the circle', async () => {
      const manifest = createManifest();
      await expect(
        service.broadcastToCircle(manifest, 'operations', { data: 'test' }),
      ).rejects.toThrow('is not a member of circle');
    });
  });

  describe('getAgentChannels', () => {
    it('returns all channels for an agent', () => {
      const manifest = createManifest();
      const channels = service.getAgentChannels(manifest);

      expect(channels.thoughts).toBe('thoughts:architect-prime');
      expect(channels.status).toBe('agent:architect-prime:status');
      expect(channels.tokens).toBe('tokens:architect-prime');
      expect(channels.dmPeers).toContain('private:architect-prime:developer-prime');
      expect(channels.circles).toEqual(['circle:development']);
    });
  });

  describe('getHistory', () => {
    it('returns an empty array on error', async () => {
      const result = await service.getHistory('test-channel');
      expect(result).toEqual([]);
    });
  });
});
