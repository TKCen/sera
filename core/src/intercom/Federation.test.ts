import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import express from 'express';
import { IntercomService } from '../intercom/IntercomService.js';
import { BridgeService } from '../intercom/BridgeService.js';
import { CircleRegistry } from '../circles/CircleRegistry.js';
import { createIntercomRouter } from '../routes/intercom.js';
import request from 'supertest';
import type { AgentManifest } from '../agents/index.js';

vi.mock('axios', () => {
  return {
    default: {
      create: vi.fn().mockImplementation(() => ({
        post: vi.fn().mockImplementation(async (url: string, data: unknown) => {
          // If the URL is for Instance B, call appB directly
          const appB = (global as unknown as { appB: express.Express }).appB;
          if (appB && url.includes('/api/intercom/bridge/receive')) {
            const res = await request(appB)
              .post('/api/intercom/bridge/receive')
              .send(data as object);
            return { data: res.body, status: res.status };
          }
          return { data: { result: {} }, status: 200 };
        }),
      })),
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
  let serverB: import('http').Server;

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
      } as unknown as import('../circles/types.js').Circle,
    ]);
    vi.spyOn(registryA, 'getCircle').mockImplementation((name) => {
      if (name === 'circle-a')
        return {
          metadata: { name: 'circle-a' },
        } as unknown as import('../circles/types.js').Circle;
      return undefined;
    });

    bridgeA.init(intercomA, registryA);
    intercomA.setBridgeService(bridgeA);

    appA = express();
    appA.use(express.json());
    appA.use(
      '/api/intercom',
      createIntercomRouter(
        intercomA,
        (name) => {
          if (name === 'agent-a')
            return {
              metadata: { name: 'agent-a', circle: 'circle-a', tier: 1 },
              intercom: { canMessage: ['*'] },
            } as AgentManifest;
          return undefined;
        },
        bridgeA
      )
    );

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
      } as unknown as import('../circles/types.js').Circle,
    ]);
    vi.spyOn(registryB, 'getCircle').mockImplementation((name) => {
      if (name === 'circle-b')
        return {
          metadata: { name: 'circle-b' },
        } as unknown as import('../circles/types.js').Circle;
      return undefined;
    });

    bridgeB.init(intercomB, registryB);
    intercomB.setBridgeService(bridgeB);

    appB = express();
    (global as unknown as { appB: express.Express }).appB = appB;
    appB.use(express.json());
    appB.use(
      '/api/intercom',
      createIntercomRouter(
        intercomB,
        (name) => {
          if (name === 'agent-b')
            return {
              metadata: { name: 'agent-b', circle: 'circle-b', tier: 1 },
            } as AgentManifest;
          return undefined;
        },
        bridgeB
      )
    );

    // Listen on a port for Instance B so A can connect
    serverB = appB.listen(0);
    const address = serverB.address() as import('net').AddressInfo;
    const port = address.port;

    // Update connection to point to the actual dynamic port
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
              endpoint: `http://localhost:${port}`,
              token: 'secret-a-to-b',
            },
          },
        ],
      } as unknown as import('../circles/types.js').Circle,
    ]);
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
    await new Promise((resolve) => setTimeout(resolve, 500));

    // Verify Instance B received the message
    expect(publishSpyB).toHaveBeenCalled();
    const [, message] = publishSpyB.mock.calls[0] as [string, unknown];
    const intercomMsg = message as import('./types.js').IntercomMessage;
    expect(intercomMsg.payload['text']).toBe('Hello Instance B!');
    expect(intercomMsg.source['agent']).toBe('agent-a');
    expect(intercomMsg.source['circle']).toBe('circle-a');
  });
});
