import { describe, it, expect, beforeEach } from 'vitest';
import { SkillRegistry } from './SkillRegistry.js';

describe('SkillRegistry - Cycle Detection', () => {
  let registry: SkillRegistry;

  beforeEach(() => {
    registry = new SkillRegistry();
  });

  it('detects direct cycles', () => {
    registry.register({
      id: 's1',
      description: 'd1',
      parameters: [],
      source: 'builtin',
      handler: async () => ({ success: true }),
      requires: ['s1'],
    });
    const manifest = {
      skills: ['s1'],
    } as unknown as import('../agents/index.js').AgentManifest;
    const errors = registry.validateManifestSkills(manifest);
    expect(errors[0]).toContain('Circular skill dependency detected: s1 -> s1');
  });

  it('detects indirect cycles', () => {
    registry.register({
      id: 's1',
      description: 'd1',
      parameters: [],
      source: 'builtin',
      handler: async () => ({ success: true }),
      requires: ['s2'],
    });
    registry.register({
      id: 's2',
      description: 'd2',
      parameters: [],
      source: 'builtin',
      handler: async () => ({ success: true }),
      requires: ['s1'],
    });
    const manifest = {
      skills: ['s1'],
    } as unknown as import('../agents/index.js').AgentManifest;
    const errors = registry.validateManifestSkills(manifest);
    expect(errors[0]).toContain('Circular skill dependency detected');
    expect(errors[0]).toMatch(/s1 -> s2 -> s1|s2 -> s1 -> s2/);
  });

  it('allows non-circular dependencies', () => {
    registry.register({
      id: 's1',
      description: 'd1',
      parameters: [],
      source: 'builtin',
      handler: async () => ({ success: true }),
      requires: ['s2'],
    });
    registry.register({
      id: 's2',
      description: 'd2',
      parameters: [],
      source: 'builtin',
      handler: async () => ({ success: true }),
    });
    const manifest = {
      skills: ['s1'],
    } as unknown as import('../agents/index.js').AgentManifest;
    const errors = registry.validateManifestSkills(manifest);
    expect(errors).toHaveLength(0);
  });
});
