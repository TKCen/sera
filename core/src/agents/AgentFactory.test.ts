import { describe, it, expect } from 'vitest';
import path from 'path';
import { AgentFactory } from './AgentFactory.js';
import type { AgentManifest } from './manifest/types.js';

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('AgentFactory', () => {
  // agents/ directory is now empty (templates moved to templates/builtin/)
  const agentsDir = path.resolve(import.meta.dirname, '..', '..', '..', 'agents');

  describe('loadTemplates', () => {
    it('should return empty map when agents directory has no YAML files', () => {
      const templates = AgentFactory.loadTemplates(agentsDir);
      expect(templates.size).toBe(0);
    });

    it('should return empty map for a non-existent directory', () => {
      const templates = AgentFactory.loadTemplates('/nonexistent/dir');
      expect(templates.size).toBe(0);
    });
  });

  describe('createAgent', () => {
    it('should create an agent from a minimal manifest', () => {
      const manifest = {
        apiVersion: 'sera/v1',
        kind: 'Agent' as const,
        metadata: { name: 'test-agent', displayName: 'Test Agent', icon: '', tier: 2 },
        identity: { role: 'Test role', description: '' },
        model: { provider: 'test', name: 'test-model' },
      } satisfies AgentManifest;

      const agent = AgentFactory.createAgent(manifest);
      expect(agent.name).toBe('Test Agent');
      // @ts-expect-error - manifest is protected but we want to check it in test
      expect(agent.manifest.metadata.name).toBe('test-agent');
    });
  });
});
