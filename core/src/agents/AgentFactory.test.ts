import { describe, it, expect } from 'vitest';
import path from 'path';
import { AgentFactory } from './AgentFactory.js';

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('AgentFactory', () => {
  const agentsDir = path.resolve(import.meta.dirname, '..', '..', '..', 'agents');

  describe('loadTemplates', () => {
    it('should load all manifests from the agents directory', () => {
      const templates = AgentFactory.loadTemplates(agentsDir);

      expect(templates.size).toBe(5);

      const names = Array.from(templates.keys()).sort();
      expect(names).toEqual(['architect-prime', 'developer-prime', 'general-assistant', 'researcher-prime', 'writer']);
    });

    it('should return empty map for a non-existent directory', () => {
      const templates = AgentFactory.loadTemplates('/nonexistent/dir');
      expect(templates.size).toBe(0);
    });
  });

  describe('createAgent', () => {
    it('should create an agent from a manifest', () => {
      const templates = AgentFactory.loadTemplates(agentsDir);
      const manifest = templates.get('architect-prime')!;

      const agent = AgentFactory.createAgent(manifest);
      expect(agent.name).toBe('Winston');
      // @ts-ignore - manifest is protected but we want to check it in test
      expect(agent.manifest.metadata.name).toBe('architect-prime');
    });
  });
});
