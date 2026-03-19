import { describe, it, expect, vi, beforeEach } from 'vitest';
import { BridgeService } from './BridgeService.js';
import { IntercomService } from './IntercomService.js';
import { CircleRegistry } from '../circles/CircleRegistry.js';
import type { IntercomMessage } from './types.js';
import axios from 'axios';

vi.mock('axios');
vi.mock('fs', () => ({
  default: {
    readFileSync: vi.fn().mockReturnValue('mock-cert-content'),
  },
}));

describe('BridgeService', () => {
  let bridge: BridgeService;
  let intercom: IntercomService;
  let registry: CircleRegistry;

  beforeEach(() => {
    vi.clearAllMocks();
    intercom = new IntercomService();
    registry = new CircleRegistry();
    bridge = new BridgeService();
    bridge.init(intercom, registry);
  });

  it('should not forward non-bridge channels', async () => {
    const postSpy = vi.spyOn(axios, 'post');
    const message: IntercomMessage = {
      id: '1',
      version: '1',
      timestamp: new Date().toISOString(),
      source: { agent: 'a', circle: 'local' },
      target: { channel: 'channel:local:updates' },
      type: 'message',
      payload: {},
      metadata: { securityTier: 1 },
    };

    await bridge.handleLocalPublish('channel:local:updates', message);
    expect(postSpy).not.toHaveBeenCalled();
  });

  it('should forward messages to remote circles', async () => {
    // Setup registry with a remote connection
    vi.spyOn(registry, 'listCircles').mockReturnValue([
      {
        apiVersion: 'sera/v1',
        kind: 'Circle',
        metadata: { name: 'local', displayName: 'Local' },
        agents: [],
        connections: [
          {
            circle: 'remote',
            auth: {
              type: 'token',
              endpoint: 'http://remote-instance/api',
              token: 'secret',
            },
          },
        ],
      },
    ]);
    vi.spyOn(registry, 'getCircle').mockImplementation((name) => {
      if (name === 'local') return { metadata: { name: 'local' } } as any;
      return undefined;
    });

    const mockAxiosInstance = {
      post: vi.fn().mockResolvedValue({ data: { success: true } }),
    };
    (axios.create as any).mockReturnValue(mockAxiosInstance);

    const message: IntercomMessage = {
      id: '2',
      version: '1',
      timestamp: new Date().toISOString(),
      source: { agent: 'a', circle: 'local' },
      target: { channel: 'bridge:local:remote:updates' },
      type: 'message',
      payload: { text: 'hello' },
      metadata: { securityTier: 1 },
    };

    await bridge.handleLocalPublish('bridge:local:remote:updates', message);

    expect(axios.create).toHaveBeenCalledWith(expect.objectContaining({
      baseURL: 'http://remote-instance/api',
      headers: expect.objectContaining({
        Authorization: 'Bearer secret',
      }),
    }));
    expect(mockAxiosInstance.post).toHaveBeenCalledWith(
      '/api/intercom/bridge/receive',
      expect.objectContaining({
        channel: 'bridge:local:remote:updates',
        message: expect.objectContaining({
          payload: { text: 'hello' },
        }),
      }),
    );
  });

  it('should handle incoming bridged messages', async () => {
    const publishSpy = vi.spyOn(intercom, 'publish').mockResolvedValue(undefined);
    const message: IntercomMessage = {
      id: '3',
      version: '1',
      timestamp: new Date().toISOString(),
      source: { agent: 'r', circle: 'remote' },
      target: { channel: 'bridge:local:remote:updates' },
      type: 'message',
      payload: { text: 'from remote' },
      metadata: { securityTier: 1 },
    };

    await bridge.receive('bridge:local:remote:updates', message);
    expect(publishSpy).toHaveBeenCalledWith('bridge:local:remote:updates', message);
  });
});
