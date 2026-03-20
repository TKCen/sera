import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SkillRegistry } from './SkillRegistry.js';
import type { SkillDefinition } from './types.js';
import type { AgentManifest } from '../agents/manifest/types.js';
import type { AgentContext } from './types.js';
import type { MCPRegistry } from '../mcp/registry.js';

// ── Helpers ─────────────────────────────────────────────────────────────────────

function dummySkill(id: string, source: 'builtin' | 'mcp' | 'custom' = 'builtin'): SkillDefinition {
  return {
    id,
    description: `Test skill: ${id}`,
    source,
    parameters: [{ name: 'input', type: 'string', description: 'Test input', required: true }],
    handler: async (params) => {
      return { success: true, data: `echo:${String(params['input'])}` };
    },
  };
}

function minimalManifest(overrides: Partial<AgentManifest> = {}): AgentManifest {
  return {
    apiVersion: 'sera/v1',
    kind: 'Agent',
    metadata: {
      name: 'test-agent',
      displayName: 'Test',
      icon: '🧪',
      circle: 'development',
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
    ...overrides,
  };
}

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('SkillRegistry', () => {
  let registry: SkillRegistry;

  beforeEach(() => {
    registry = new SkillRegistry();
  });

  // ── Registration & Lookup ─────────────────────────────────────────────────

  describe('register / get / has', () => {
    it('should register and retrieve a skill by ID', () => {
      registry.register(dummySkill('my-skill'));
      expect(registry.has('my-skill')).toBe(true);
      expect(registry.get('my-skill')?.id).toBe('my-skill');
    });

    it('should return undefined for unknown skill', () => {
      expect(registry.get('nonexistent')).toBeUndefined();
      expect(registry.has('nonexistent')).toBe(false);
    });

    it('should overwrite a skill with the same ID', () => {
      registry.register(dummySkill('x'));
      registry.register({ ...dummySkill('x'), description: 'updated' });
      expect(registry.get('x')?.description).toBe('updated');
    });

    it('should unregister a skill', () => {
      registry.register(dummySkill('y'));
      expect(registry.unregister('y')).toBe(true);
      expect(registry.has('y')).toBe(false);
    });

    it('should return false when unregistering unknown skill', () => {
      expect(registry.unregister('nope')).toBe(false);
    });
  });

  // ── Listing ───────────────────────────────────────────────────────────────

  describe('listAll', () => {
    it('should return all registered skills as SkillInfo (no handler)', () => {
      registry.register(dummySkill('a'));
      registry.register(dummySkill('b', 'mcp'));
      const list = registry.listAll();
      expect(list).toHaveLength(2);
      expect(list.map((s) => s.id).sort()).toEqual(['a', 'b']);
      // SkillInfo should not have handler
      for (const info of list) {
        expect(info).not.toHaveProperty('handler');
      }
    });
  });

  describe('listForAgent', () => {
    it('should return skills referenced by manifest skills array', () => {
      registry.register(dummySkill('skill-a'));
      registry.register(dummySkill('skill-b'));
      registry.register(dummySkill('skill-c'));

      const manifest = minimalManifest({ skills: ['skill-a', 'skill-b'] });
      const list = registry.listForAgent(manifest);
      expect(list.map((s) => s.id).sort()).toEqual(['skill-a', 'skill-b']);
    });

    it('should include tools.allowed that are registered as skills', () => {
      registry.register(dummySkill('file-read'));
      registry.register(dummySkill('web-search'));

      const manifest = minimalManifest({
        tools: { allowed: ['file-read', 'web-search'] },
      });
      const list = registry.listForAgent(manifest);
      expect(list.map((s) => s.id).sort()).toEqual(['file-read', 'web-search']);
    });

    it('should subtract tools.denied from the list', () => {
      registry.register(dummySkill('file-read'));
      registry.register(dummySkill('web-search'));

      const manifest = minimalManifest({
        tools: { allowed: ['file-read', 'web-search'], denied: ['web-search'] },
      });
      const list = registry.listForAgent(manifest);
      expect(list.map((s) => s.id)).toEqual(['file-read']);
    });

    it('should not include unregistered skills from manifest', () => {
      registry.register(dummySkill('exists'));

      const manifest = minimalManifest({ skills: ['exists', 'does-not-exist'] });
      const list = registry.listForAgent(manifest);
      expect(list.map((s) => s.id)).toEqual(['exists']);
    });
  });

  // ── Invocation ────────────────────────────────────────────────────────────

  describe('invoke', () => {
    it('should invoke a registered skill and return its result', async () => {
      registry.register(dummySkill('echo'));
      const result = await registry.invoke(
        'echo',
        { input: 'hello' },
        {} as unknown as AgentContext
      );
      expect(result.success).toBe(true);
      expect(result.data).toBe('echo:hello');
    });

    it('should return error for unknown skill', async () => {
      const result = await registry.invoke('unknown', {}, {} as unknown as AgentContext);
      expect(result.success).toBe(false);
      expect(result.error).toContain('not found');
    });

    it('should catch handler exceptions and return error', async () => {
      registry.register({
        ...dummySkill('broken'),
        handler: async () => {
          throw new Error('boom');
        },
      });
      const result = await registry.invoke('broken', {}, {} as unknown as AgentContext);
      expect(result.success).toBe(false);
      expect(result.error).toBe('boom');
    });
  });

  // ── Composition ───────────────────────────────────────────────────────────

  describe('skill composition', () => {
    it('should allow a skill to invoke another skill via the composition callback', async () => {
      registry.register(dummySkill('inner'));
      registry.register({
        id: 'outer',
        description: 'Calls inner skill',
        source: 'builtin',
        parameters: [],
        handler: async (_params, context, invoke) => {
          const inner = await invoke!('inner', { input: 'composed' }, context);
          return { success: true, data: `outer(${String(inner.data)})` };
        },
      });

      const result = await registry.invoke('outer', {}, {} as unknown as AgentContext);
      expect(result.success).toBe(true);
      expect(result.data).toBe('outer(echo:composed)');
    });
  });

  // ── Validation ────────────────────────────────────────────────────────────

  describe('validateManifestSkills', () => {
    it('should return empty for valid manifest', () => {
      registry.register(dummySkill('skill-a'));
      registry.register(dummySkill('file-read'));

      const manifest = minimalManifest({
        skills: ['skill-a'],
        tools: { allowed: ['file-read'] },
      });
      expect(registry.validateManifestSkills(manifest)).toEqual([]);
    });

    it('should return unknown skill IDs', () => {
      registry.register(dummySkill('known'));

      const manifest = minimalManifest({
        skills: ['known', 'unknown-skill'],
        tools: { allowed: ['unknown-tool'] },
      });
      const unknown = registry.validateManifestSkills(manifest);
      expect(unknown.sort()).toEqual(['unknown-skill', 'unknown-tool']);
    });

    it('should handle manifest with no skills or tools', () => {
      const manifest = minimalManifest();
      expect(registry.validateManifestSkills(manifest)).toEqual([]);
    });
  });

  // ── MCP Bridge ────────────────────────────────────────────────────────────

  describe('bridgeMCPTools', () => {
    it('should wrap MCP tools as skills', async () => {
      const mockCallTool = vi.fn().mockResolvedValue({ content: [{ text: 'result' }] });

      const mockMCPRegistry = {
        getAllTools: vi.fn().mockResolvedValue([
          {
            serverName: 'test-server',
            tools: [
              {
                name: 'mcp-tool-a',
                description: 'A mock MCP tool',
                inputSchema: {
                  type: 'object',
                  properties: {
                    query: { type: 'string', description: 'Search query' },
                    count: { type: 'integer', description: 'How many' },
                  },
                  required: ['query'],
                },
              },
            ],
          },
        ]),
        getClient: vi.fn().mockReturnValue({
          callTool: mockCallTool,
          listTools: vi.fn().mockResolvedValue({
            tools: [
              {
                name: 'mcp-tool-a',
                description: 'A mock MCP tool',
                inputSchema: {
                  type: 'object',
                  properties: {
                    query: { type: 'string', description: 'Search query' },
                    count: { type: 'integer', description: 'How many' },
                  },
                  required: ['query'],
                },
              },
            ],
          }),
        }),
        getClients: vi.fn().mockReturnValue(
          new Map([
            [
              'test-server',
              {
                callTool: mockCallTool,
                listTools: vi.fn().mockResolvedValue({
                  tools: [
                    {
                      name: 'mcp-tool-a',
                      description: 'A mock MCP tool',
                      inputSchema: {
                        type: 'object',
                        properties: {
                          query: { type: 'string', description: 'Search query' },
                          count: { type: 'integer', description: 'How many' },
                        },
                        required: ['query'],
                      },
                    },
                  ],
                }),
              },
            ],
          ])
        ),
      };

      const count = await registry.bridgeMCPTools(mockMCPRegistry as unknown as MCPRegistry);
      expect(count).toBe(1);
      expect(registry.has('test-server/mcp-tool-a')).toBe(true);

      const skill = registry.get('test-server/mcp-tool-a')!;
      expect(skill.source).toBe('mcp');
      expect(skill.parameters).toHaveLength(2);
      expect(skill.parameters[0]).toEqual({
        name: 'query',
        type: 'string',
        description: 'Search query',
        required: true,
      });
      expect(skill.parameters[1]).toEqual({
        name: 'count',
        type: 'number',
        description: 'How many',
        required: false,
      });

      // Invoke the bridged skill
      const result = await registry.invoke('test-server/mcp-tool-a', { query: 'test' }, {
        manifest: { metadata: { name: 'test' } },
      } as unknown as AgentContext);
      expect(result.success).toBe(true);
      expect(mockCallTool).toHaveBeenCalledWith('mcp-tool-a', { query: 'test' }, expect.anything());
    });

    it('should handle MCP tool invocation errors gracefully', async () => {
      const mockCallTool = vi.fn().mockRejectedValue(new Error('connection lost'));
      const mockMCPRegistry = {
        getAllTools: vi.fn().mockResolvedValue([
          {
            serverName: 'err-server',
            tools: [{ name: 'fail-tool', description: 'Will fail', inputSchema: {} }],
          },
        ]),
        getClient: vi.fn().mockReturnValue({
          callTool: mockCallTool,
          listTools: vi.fn().mockResolvedValue({
            tools: [{ name: 'fail-tool', description: 'Will fail', inputSchema: {} }],
          }),
        }),
        getClients: vi.fn().mockReturnValue(
          new Map([
            [
              'err-server',
              {
                callTool: mockCallTool,
                listTools: vi.fn().mockResolvedValue({
                  tools: [{ name: 'fail-tool', description: 'Will fail', inputSchema: {} }],
                }),
              },
            ],
          ])
        ),
      };

      await registry.bridgeMCPTools(mockMCPRegistry as unknown as MCPRegistry);
      const result = await registry.invoke('err-server/fail-tool', {}, {
        manifest: { metadata: { name: 'test' } },
      } as unknown as AgentContext);
      expect(result.success).toBe(false);
      expect(result.error).toBe('connection lost');
    });

    it('should skip servers where getClient returns undefined', async () => {
      const mockMCPRegistry = {
        getAllTools: vi
          .fn()
          .mockResolvedValue([
            { serverName: 'gone', tools: [{ name: 'ghost', description: '', inputSchema: {} }] },
          ]),
        getClient: vi.fn().mockReturnValue(undefined),
        getClients: vi.fn().mockReturnValue(new Map()),
      };

      const count = await registry.bridgeMCPTools(mockMCPRegistry as unknown as MCPRegistry);
      expect(count).toBe(0);
      expect(registry.has('ghost')).toBe(false);
    });
  });
});
