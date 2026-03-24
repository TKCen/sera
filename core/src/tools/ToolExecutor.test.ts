import { describe, it, expect, vi } from 'vitest';
import { ToolExecutor } from './ToolExecutor.js';
import type { SkillRegistry } from '../skills/SkillRegistry.js';

vi.mock('../audit/AuditService.js', () => ({
  AuditService: {
    getInstance: () => ({
      record: vi.fn().mockResolvedValue(undefined),
    }),
  },
}));

import type { AgentManifest } from '../agents/manifest/types.js';
import type { SkillInfo, SkillResult } from '../skills/types.js';
import type { ToolCall } from '../lib/llm/types.js';

// ── Helpers ─────────────────────────────────────────────────────────────────────

function minimalManifest(): AgentManifest {
  return {
    apiVersion: 'sera/v1',
    kind: 'Agent',
    metadata: {
      name: 'test-agent',
      displayName: 'Test Agent',
      icon: '🧪',
      circle: 'test',
      tier: 2,
    },
    identity: {
      role: 'Tester',
      description: 'A test agent',
    },
    model: {
      provider: 'lm-studio',
      name: 'test-model',
    },
    tools: {
      allowed: ['*'],
    },
  };
}

function createMockRegistry(skills: SkillInfo[] = [], invokeResult?: SkillResult): SkillRegistry {
  return {
    listForAgent: vi.fn().mockReturnValue(skills),
    invoke: vi.fn().mockResolvedValue(invokeResult ?? { success: true, data: 'mock result' }),
    // Other methods not needed by ToolExecutor
    register: vi.fn(),
    unregister: vi.fn(),
    get: vi.fn(),
    has: vi.fn(),
    listAll: vi.fn(),
    validateManifestSkills: vi.fn(),
    bridgeMCPTools: vi.fn(),
  } as unknown as SkillRegistry;
}

function makeToolCall(name: string, args: Record<string, unknown>, id = 'tc-1'): ToolCall {
  return {
    id,
    type: 'function',
    function: {
      name,
      arguments: JSON.stringify(args),
    },
  };
}

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('ToolExecutor', () => {
  // ── getToolDefinitions ──────────────────────────────────────────────────

  describe('getToolDefinitions', () => {
    it('should convert skills into OpenAI tool definitions', () => {
      const skills: SkillInfo[] = [
        {
          id: 'web-search',
          description: 'Search the web',
          source: 'builtin',
          parameters: [
            { name: 'query', type: 'string', description: 'Search query', required: true },
            { name: 'limit', type: 'number', description: 'Max results', required: false },
          ],
        },
        {
          id: 'file-read',
          description: 'Read a file',
          source: 'builtin',
          parameters: [{ name: 'path', type: 'string', description: 'File path', required: true }],
        },
      ];

      const registry = createMockRegistry(skills);
      const executor = new ToolExecutor(registry);
      const manifest = minimalManifest();

      const defs = executor.getToolDefinitions(manifest);

      expect(defs).toHaveLength(2);

      // First tool
      expect(defs[0]!.type).toBe('function');
      expect(defs[0]!.function.name).toBe('web-search');
      expect(defs[0]!.function.description).toBe('Search the web');
      expect(defs[0]!.function.parameters).toEqual({
        type: 'object',
        properties: {
          query: { type: 'string', description: 'Search query' },
          limit: { type: 'number', description: 'Max results' },
        },
        required: ['query'],
      });

      // Second tool
      expect(defs[1]!.function.name).toBe('file-read');
      expect(defs[1]!.function.parameters).toEqual({
        type: 'object',
        properties: {
          path: { type: 'string', description: 'File path' },
        },
        required: ['path'],
      });
    });

    it('should return empty array when agent has no skills', () => {
      const registry = createMockRegistry([]);
      const executor = new ToolExecutor(registry);
      const defs = executor.getToolDefinitions(minimalManifest());
      expect(defs).toEqual([]);
    });

    it('should omit required array when no params are required', () => {
      const skills: SkillInfo[] = [
        {
          id: 'optional-tool',
          description: 'All optional',
          source: 'builtin',
          parameters: [{ name: 'foo', type: 'string', description: 'Optional', required: false }],
        },
      ];

      const registry = createMockRegistry(skills);
      const executor = new ToolExecutor(registry);
      const defs = executor.getToolDefinitions(minimalManifest());

      expect(defs[0]!.function.parameters).toEqual({
        type: 'object',
        properties: {
          foo: { type: 'string', description: 'Optional' },
        },
      });
      // No "required" key
      expect(defs[0]!.function.parameters).not.toHaveProperty('required');
    });
  });

  // ── executeTool ─────────────────────────────────────────────────────────

  describe('executeTool', () => {
    it('should execute a tool call and return a tool-role ChatMessage', async () => {
      const registry = createMockRegistry([], { success: true, data: { greeting: 'hello' } });
      const executor = new ToolExecutor(registry);

      const toolCall = makeToolCall('test-skill', { input: 'world' });
      const result = await executor.executeTool(toolCall, minimalManifest());

      expect(result.role).toBe('tool');
      expect(result.tool_call_id).toBe('tc-1');
      expect(result.content).toContain('hello');
      expect(registry.invoke).toHaveBeenCalledWith(
        'test-skill',
        { input: 'world' },
        {
          agentName: 'test-agent',
          workspacePath: 'workspaces/test-agent',
          tier: 2,
          manifest: minimalManifest(),
          agentInstanceId: undefined,
          containerId: undefined,
          sessionId: 'default',
          sandboxManager: undefined,
          allowedPaths: ['/workspace', '/memory', '/knowledge'],
        }
      );
    });

    it('should return string data as-is', async () => {
      const registry = createMockRegistry([], { success: true, data: 'plain string result' });
      const executor = new ToolExecutor(registry);

      const result = await executor.executeTool(makeToolCall('test', {}), minimalManifest());
      expect(result.content).toBe('plain string result');
    });

    it('should return error message on skill failure', async () => {
      const registry = createMockRegistry([], { success: false, error: 'Skill failed' });
      const executor = new ToolExecutor(registry);

      const result = await executor.executeTool(makeToolCall('test', {}), minimalManifest());
      expect(result.content).toBe('Error: Skill failed');
    });

    it('should return error for invalid JSON arguments', async () => {
      const registry = createMockRegistry([]);
      const executor = new ToolExecutor(registry);

      const toolCall: ToolCall = {
        id: 'tc-bad',
        type: 'function',
        function: {
          name: 'test',
          arguments: '{invalid json',
        },
      };

      const result = await executor.executeTool(toolCall, minimalManifest());
      expect(result.content).toContain('Failed to parse tool arguments');
    });

    it('should handle tool arguments wrapped in markdown', async () => {
      const registry = createMockRegistry([], { success: true, data: 'parsed' });
      const executor = new ToolExecutor(registry);

      const toolCall: ToolCall = {
        id: 'tc-md',
        type: 'function',
        function: {
          name: 'test',
          arguments: '```json\n{"foo": "bar"}\n```',
        },
      };

      const result = await executor.executeTool(toolCall, minimalManifest());
      expect(result.role).toBe('tool');
      expect(result.content).toBe('parsed');
      expect(registry.invoke).toHaveBeenCalledWith('test', { foo: 'bar' }, expect.any(Object));
    });

    it('should handle tool arguments with extra text', async () => {
      const registry = createMockRegistry([], { success: true, data: 'parsed' });
      const executor = new ToolExecutor(registry);

      const toolCall: ToolCall = {
        id: 'tc-text',
        type: 'function',
        function: {
          name: 'test',
          arguments: 'The arguments are: {"foo": "bar"}',
        },
      };

      const result = await executor.executeTool(toolCall, minimalManifest());
      expect(result.role).toBe('tool');
      expect(result.content).toBe('parsed');
      expect(registry.invoke).toHaveBeenCalledWith('test', { foo: 'bar' }, expect.any(Object));
    });

    it('should truncate results exceeding 50K characters', async () => {
      const longData = 'x'.repeat(60_000);
      const registry = createMockRegistry([], { success: true, data: longData });
      const executor = new ToolExecutor(registry);

      const result = await executor.executeTool(makeToolCall('test', {}), minimalManifest());
      expect(result.content.length).toBeLessThan(60_000);
      expect(result.content).toContain('[TRUNCATED');
    });

    it('should handle thrown exceptions in skill execution', async () => {
      const registry = createMockRegistry([]);
      vi.mocked(registry.invoke).mockRejectedValue(new Error('Unexpected crash'));
      const executor = new ToolExecutor(registry);

      const result = await executor.executeTool(makeToolCall('test', {}), minimalManifest());
      expect(result.role).toBe('tool');
      expect(result.content).toContain('Unexpected crash');
    });
  });

  // ── executeToolCalls ───────────────────────────────────────────────────

  describe('executeToolCalls', () => {
    it('should execute multiple tool calls in parallel', async () => {
      const registry = createMockRegistry([], { success: true, data: 'ok' });
      const executor = new ToolExecutor(registry);

      const calls = [
        makeToolCall('skill-a', { x: 1 }, 'tc-1'),
        makeToolCall('skill-b', { y: 2 }, 'tc-2'),
      ];

      const results = await executor.executeToolCalls(calls, minimalManifest());

      expect(results).toHaveLength(2);
      expect(results[0]!.tool_call_id).toBe('tc-1');
      expect(results[1]!.tool_call_id).toBe('tc-2');
      expect(registry.invoke).toHaveBeenCalledTimes(2);
    });
  });
});
