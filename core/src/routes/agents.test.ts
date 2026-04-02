import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import express from 'express';
import { createAgentRouter } from './agents.js';

describe('Agents Routes', () => {
  let app: express.Express;
  let orchestratorMock: any;
  let agentRegistryMock: any;
  let skillRegistryMock: any;
  let intercomMock: any;

  beforeEach(() => {
    vi.unstubAllGlobals();

    intercomMock = {
      publish: vi.fn(),
      getThoughts: vi.fn(),
    };

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
    };

    agentRegistryMock = {
      listInstances: vi.fn().mockResolvedValue([]),
      listTemplates: vi.fn().mockResolvedValue([]),
      getTemplate: vi.fn(),
      createInstance: vi.fn(),
      getInstance: vi.fn(),
      updateInstanceStatus: vi.fn(),
      deleteInstance: vi.fn(),
    };

    skillRegistryMock = {
      listForAgent: vi.fn(),
      validateManifestSkills: vi.fn(),
    };

    app = express();
    app.use(express.json());
    app.use(
      '/api/agents',
      createAgentRouter(orchestratorMock, agentRegistryMock, skillRegistryMock)
    );
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
      expect(intercomMock.publish.mock.calls[0][1].durationMs).toBeDefined();
    });
  });

  describe('GET /api/agents/instances/:id/tools', () => {
    it('returns available and unavailable tools for an agent instance', async () => {
      const instanceId = 'agent-123';
      const mockInstance = {
        id: instanceId,
        name: 'test-agent',
        template_ref: 'test-template',
      };
      const mockManifest = {
        metadata: { name: 'test-agent' },
        tools: { allowed: ['tool1', 'tool2'] },
      };

      agentRegistryMock.getInstance.mockResolvedValue(mockInstance);
      orchestratorMock.getManifestByInstanceId.mockReturnValue(mockManifest);
      skillRegistryMock.listForAgent.mockReturnValue([{ id: 'tool1', description: 'Tool 1' }]);
      skillRegistryMock.validateManifestSkills.mockReturnValue(['tool2']);

      const res = await request(app).get(`/api/agents/instances/${instanceId}/tools`);

      expect(res.status).toBe(200);
      expect(res.body).toEqual({
        available: [{ id: 'tool1', description: 'Tool 1' }],
        unavailable: ['tool2'],
      });
      expect(skillRegistryMock.listForAgent).toHaveBeenCalledWith(mockManifest);
      expect(skillRegistryMock.validateManifestSkills).toHaveBeenCalledWith(mockManifest);
    });

    it('returns 404 if agent instance not found', async () => {
      agentRegistryMock.getInstance.mockResolvedValue(null);
      const res = await request(app).get('/api/agents/instances/non-existent/tools');
      expect(res.status).toBe(404);
      expect(res.body.error).toBe('Agent instance not found');
    });

    it('returns 404 if manifest cannot be resolved', async () => {
      agentRegistryMock.getInstance.mockResolvedValue({ id: 'id', name: 'name' });
      orchestratorMock.getManifestByInstanceId.mockReturnValue(null);
      orchestratorMock.getManifest.mockReturnValue(null);
      agentRegistryMock.getTemplate.mockResolvedValue(null);

      const res = await request(app).get('/api/agents/instances/id/tools');
      expect(res.status).toBe(404);
      expect(res.body.error).toBe('Agent manifest not found');
    });
  });
});
