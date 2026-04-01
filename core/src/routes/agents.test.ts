import { describe, it, expect, vi, beforeEach, type Mocked } from 'vitest';
import request from 'supertest';
import express from 'express';
import { createAgentRouter } from './agents.js';
import type { Orchestrator } from '../agents/Orchestrator.js';
import type { AgentRegistry } from '../agents/registry.service.js';
import type { IntercomService } from '../intercom/IntercomService.js';

describe('Agents Routes', () => {
  let app: express.Express;
  let orchestratorMock: Mocked<Orchestrator>;
  let agentRegistryMock: Mocked<AgentRegistry>;
  let intercomMock: Mocked<IntercomService>;

  beforeEach(() => {
    vi.unstubAllGlobals();

    intercomMock = {
      publish: vi.fn(),
      getThoughts: vi.fn(),
    } as unknown as Mocked<IntercomService>;

    orchestratorMock = {
      startInstance: vi.fn(),
      stopInstance: vi.fn(),
      listAgents: vi.fn().mockReturnValue([]),
      getManifest: vi.fn(),
      getManifestByInstanceId: vi.fn(),
      getToolExecutor: vi.fn(),
      getIntercom: vi.fn().mockReturnValue(intercomMock),
      ensureContainerRunning: vi.fn().mockResolvedValue('http://mock-container:8080'),
      registerEphemeralTTL: vi.fn(),
    } as unknown as Mocked<Orchestrator>;

    agentRegistryMock = {
      listInstances: vi.fn().mockResolvedValue([]),
      listTemplates: vi.fn().mockResolvedValue([]),
      getTemplate: vi.fn(),
      createInstance: vi.fn(),
      getInstance: vi.fn(),
      updateInstanceStatus: vi.fn(),
      deleteInstance: vi.fn(),
    } as unknown as Mocked<AgentRegistry>;

    app = express();
    app.use(express.json());
    app.use('/api/agents', createAgentRouter(orchestratorMock, agentRegistryMock));
  });

  describe('POST /api/agents/spawn-ephemeral', () => {
    it('creates instance, spawns container, publishes result to Centrifugo when async=true', async () => {
      const templateRef = 'my-template';
      const task = 'hello world';
      const instanceId = 'ephemeral-123';

      agentRegistryMock.getTemplate.mockResolvedValue({ name: templateRef });
      agentRegistryMock.createInstance.mockResolvedValue({
        id: instanceId,
        name: 'ephemeral-name',
      });

      // Mock fetch for the container's chat endpoint
      const mockFetch = vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({
          result: 'Task completed',
          usage: { promptTokens: 10, completionTokens: 20, totalTokens: 30 },
        }),
      });
      vi.stubGlobal('fetch', mockFetch);

      const res = await request(app).post('/api/agents/spawn-ephemeral').send({
        templateRef,
        task,
        async: true,
      });

      expect(res.status).toBe(202);
      expect(res.body.instanceId).toBe(instanceId);

      // wait for async background execution to finish
      await new Promise(process.nextTick);
      await new Promise(process.nextTick);

      expect(agentRegistryMock.updateInstanceStatus).toHaveBeenCalledWith(instanceId, 'completed');
      expect(intercomMock.publish).toHaveBeenCalledWith(
        `ephemeral:${instanceId}:result`,
        expect.objectContaining({
          instanceId,
          status: 'completed',
          result: 'Task completed',
          usage: { promptTokens: 10, completionTokens: 20, totalTokens: 30 },
        })
      );
      const publishCalls = vi.mocked(intercomMock.publish).mock.calls;
      expect((publishCalls[0]![1] as Record<string, unknown>).durationMs).toBeDefined();
    });
  });
});
