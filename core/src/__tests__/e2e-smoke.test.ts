import { describe, it, expect, beforeAll } from 'vitest';
import axios, { type AxiosInstance } from 'axios';

const API_URL = process.env['SERA_API_URL'] ?? 'http://localhost:3001';
const API_KEY = process.env['SERA_API_KEY'] ?? 'sera_bootstrap_dev_123';

describe('V1 Gate — E2E Smoke Tests', () => {
  let api: AxiosInstance;

  beforeAll(() => {
    api = axios.create({
      baseURL: API_URL,
      headers: { Authorization: `Bearer ${API_KEY}` },
      timeout: 180_000, // 3 min — LLM calls can be slow
    });
  });

  // Test 1: Health check — all services up
  it('Test 1: All services healthy', async () => {
    const res = await api.get('/api/health');
    expect(res.status).toBe(200);
    expect(res.data).toHaveProperty('status');
    // Verify core services are reachable
    expect(res.data.status).toMatch(/ok|healthy|degraded/);
    // Check sub-services if available (detail endpoint returns components array)
    if (res.data.components) {
      const dbComponent = (res.data.components as Array<{ name: string }>).find(
        (c) => c.name === 'database'
      );
      expect(dbComponent).toBeDefined();
    }
  });

  // Test 2: Basic chat — send a message, get a response
  it('Test 2: Basic chat with Sera agent', async () => {
    // First, find the Sera agent instance
    const agents = await api.get('/api/agents');
    expect(agents.status).toBe(200);
    expect(Array.isArray(agents.data)).toBe(true);
    // There should be at least one agent
    expect((agents.data as unknown[]).length).toBeGreaterThan(0);

    const agentList = agents.data as Array<{ id: string; name: string; template_ref?: string }>;
    const seraAgent = agentList.find((a) => a.name === 'sera' || a.template_ref === 'sera');
    const targetAgent = seraAgent ?? agentList[0]!;

    // Send a chat message using agentInstanceId (the correct body field)
    const chatRes = await api.post('/api/chat', {
      agentInstanceId: targetAgent.id,
      message: 'Hello! Please respond with exactly: "Smoke test OK"',
    });
    expect(chatRes.status).toBe(200);
    // Response should contain some content
    expect(chatRes.data).toBeDefined();
    const data = chatRes.data as Record<string, unknown>;
    const responseText =
      typeof chatRes.data === 'string'
        ? chatRes.data
        : String(
            data['reply'] ?? data['result'] ?? data['content'] ?? JSON.stringify(chatRes.data)
          );
    expect(responseText.length).toBeGreaterThan(0);
  }, 180_000);

  // Test 3: Tool listing — verify tools are registered and accessible
  it('Test 3: Tool listing available', async () => {
    const toolsRes = await api.get('/api/tools');
    expect(toolsRes.status).toBe(200);
    expect(Array.isArray(toolsRes.data)).toBe(true);
    expect((toolsRes.data as unknown[]).length).toBeGreaterThan(0);

    // Verify essential tool categories exist
    const tools = toolsRes.data as Array<{ id?: string; name?: string }>;
    const toolNames = tools.map((t) => t.name ?? t.id ?? '');
    // At minimum, knowledge tools should be present
    const hasKnowledgeTool = toolNames.some((n) => n.includes('knowledge') || n.includes('memory'));
    expect(hasKnowledgeTool).toBe(true);
  });

  // Test 4: Error handling — graceful response on invalid requests
  it('Test 4: Error handling returns structured errors', async () => {
    // Send to a non-existent agent — should get a clear error, not a crash
    try {
      await api.post('/api/chat', {
        agentInstanceId: 'nonexistent-agent-00000000-0000-0000-0000-000000000000',
        message: 'This should fail gracefully',
      });
      // If it doesn't throw, the response should still be defined (defensive)
    } catch (err: unknown) {
      const axiosErr = err as { response?: { status: number; data: unknown } };
      // Should get a 4xx/5xx, not a connection error
      expect(axiosErr.response).toBeDefined();
      expect(axiosErr.response!.status).toBeGreaterThanOrEqual(400);
      expect(axiosErr.response!.status).toBeLessThan(600);
      // Error response should be JSON with a message
      expect(axiosErr.response!.data).toBeDefined();
      const errData = axiosErr.response!.data as Record<string, unknown>;
      expect(errData['error'] ?? errData['message']).toBeDefined();
    }

    // Invalid endpoint — should return 4xx, not crash
    const notFound = await api
      .get('/api/nonexistent-endpoint')
      .catch((e: { response?: { status: number } }) => e.response);
    expect(notFound).toBeDefined();
    expect(notFound!.status).toBeGreaterThanOrEqual(400);
  });

  // Test 5: Memory persistence — core memory blocks CRUD
  it('Test 5: Core memory blocks CRUD', async () => {
    // List agents to get a valid instance ID
    const agents = await api.get('/api/agents');
    const agentList = agents.data as Array<{ id: string }>;
    const agentId = agentList[0]!.id;
    expect(agentId).toBeDefined();

    // List core memory blocks for the agent
    const blocksRes = await api.get(`/api/memory/${agentId}/core`);
    expect(blocksRes.status).toBe(200);
    expect(Array.isArray(blocksRes.data)).toBe(true);

    const blocks = blocksRes.data as Array<{
      name: string;
      content: string;
      characterLimit?: number;
    }>;

    // If blocks exist, verify structure
    if (blocks.length > 0) {
      const block = blocks[0]!;
      expect(block).toHaveProperty('name');
      expect(block).toHaveProperty('content');
      expect(block).toHaveProperty('characterLimit');
    }

    // Update a memory block using PATCH with action (if persona block exists)
    const personaBlock = blocks.find((b) => b.name === 'persona');
    if (personaBlock) {
      // Append to the block using the PATCH action API
      const appendRes = await api.patch(`/api/memory/${agentId}/core/persona`, {
        action: 'append',
        content: '\n[E2E smoke test marker]',
      });
      expect(appendRes.status).toBe(200);

      // Verify it persisted
      const verifyRes = await api.get(`/api/memory/${agentId}/core`);
      const updatedBlocks = verifyRes.data as Array<{ name: string; content: string }>;
      const updatedBlock = updatedBlocks.find((b) => b.name === 'persona');
      expect(updatedBlock?.content).toContain('[E2E smoke test marker]');

      // Clean up — remove the marker via replace action
      await api.patch(`/api/memory/${agentId}/core/persona`, {
        action: 'replace',
        oldText: '\n[E2E smoke test marker]',
        newText: '',
      });
    }
  });
});
