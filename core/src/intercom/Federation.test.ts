import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import express from 'express';
import { IntercomService } from '../intercom/IntercomService.js';
import { BridgeService } from '../intercom/BridgeService.js';
import { CircleRegistry } from '../circles/CircleRegistry.js';
import { createIntercomRouter } from '../routes/intercom.js';
import request from 'supertest';
import type { AgentManifest } from '../agents/manifest/types.js';
import axios from 'axios';

vi.mock('axios', async (importOriginal) => {
  const actual: any = await importOriginal();
  return {
    ...actual,
    default: {
      ...actual.default,
      create: vi.fn((config) => {
        const instance = actual.default.create(config);
        instance.post = vi.fn().mockImplementation((url, data) => {
          if (url === '' && (data?.method === 'publish' || data?.method === 'presence' || data?.method === 'history')) {
            return Promise.resolve({ data: { result: {} } });
          }
          // Use real axios for actual HTTP calls to localhost
          // BridgeService calls client.post('/api/intercom/bridge/receive', ...)
          // baseURL is 'http://localhost:3002/api/intercom'
          const fullUrl = url.startsWith('http') ? url : (config.baseURL || '') + url;
          // Log for debugging
          // console.log(`Mock axios posting to: ${fullUrl}`);
          return actual.default.post(fullUrl, data, config);
        });
        return instance;
      }),
    },
  };
});

describe('Federation Verification', () => {
  let appA: express.Express;
  let appB: express.Express;
  let intercomA: IntercomService;
  let intercomB: IntercomService;
  let bridgeA: BridgeService;
  let bridgeB: BridgeService;
  let serverB: any;

  beforeEach(async () => {
    // Instance A Setup
    intercomA = new IntercomService();
    bridgeA = new BridgeService();
    const registryA = new CircleRegistry();

    // Simulate local circle with remote connection
    vi.spyOn(registryA, 'listCircles').mockReturnValue([
      {
        apiVersion: 'sera/v1',
        kind: 'Circle',
        metadata: { name: 'circle-a', displayName: 'Circle A' },
        agents: ['agent-a'],
        connections: [
          {
            circle: 'circle-b',
            auth: {
              type: 'token',
              endpoint: 'http://localhost:3002',
              token: 'secret-a-to-b',
            },
          },
        ],
      },
    ]);
    vi.spyOn(registryA, 'getCircle').mockImplementation((name) => {
      if (name === 'circle-a') return { metadata: { name: 'circle-a' } } as any;
      return undefined;
    });

    bridgeA.init(intercomA, registryA);
    intercomA.setBridgeService(bridgeA);

    appA = express();
    appA.use(express.json());
    appA.use('/api/intercom', createIntercomRouter(intercomA, (name) => {
      if (name === 'agent-a') return {
        metadata: { name: 'agent-a', circle: 'circle-a', tier: 1 },
        intercom: { canMessage: ['*'] }
      } as AgentManifest;
      return undefined;
    }, bridgeA));

    // Instance B Setup
    intercomB = new IntercomService();
    bridgeB = new BridgeService();
    const registryB = new CircleRegistry();

    // Simulate local circle
    vi.spyOn(registryB, 'listCircles').mockReturnValue([
      {
        apiVersion: 'sera/v1',
        kind: 'Circle',
        metadata: { name: 'circle-b', displayName: 'Circle B' },
        agents: ['agent-b'],
      },
    ]);
    vi.spyOn(registryB, 'getCircle').mockImplementation((name) => {
      if (name === 'circle-b') return { metadata: { name: 'circle-b' } } as any;
      return undefined;
    });

    bridgeB.init(intercomB, registryB);
    intercomB.setBridgeService(bridgeB);

    appB = express();
    appB.use(express.json());
    appB.use('/api/intercom', createIntercomRouter(intercomB, (name) => {
      if (name === 'agent-b') return {
        metadata: { name: 'agent-b', circle: 'circle-b', tier: 1 },
      } as AgentManifest;
      return undefined;
    }, bridgeB));

    // Listen on a port for Instance B so A can connect
    serverB = appB.listen(3002);
  });

  afterEach(() => {
    if (serverB) serverB.close();
  });

  it('passes a message from Instance A to Instance B', async () => {
    // Intercept publication on Instance B to verify it arrives
    const publishSpyB = vi.spyOn(intercomB, 'publish').mockResolvedValue(undefined);

    // Send a message from Instance A to a remote agent on Instance B
    const response = await request(appA)
      .post('/api/intercom/dm')
      .send({
        from: 'agent-a',
        to: 'agent-b@circle-b',
        payload: { text: 'Hello Instance B!' },
      });

    expect(response.status).toBe(200);

    // Give some time for the bridge request to complete
    await new Promise(resolve => setTimeout(resolve, 500));

    // Verify Instance B received the message
    expect(publishSpyB).toHaveBeenCalled();
    const [channel, message] = publishSpyB.mock.calls[0] as [string, any];
    expect(channel).toBe('bridge:dm:circle-a:circle-b:agent-a:agent-b');
    expect(message.payload.text).toBe('Hello Instance B!');
    expect(message.source.agent).toBe('agent-a');
    expect(message.source.circle).toBe('circle-a');
  });
});
