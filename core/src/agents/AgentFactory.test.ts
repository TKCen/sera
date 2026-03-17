import { describe, it, expect } from 'vitest';
import path from 'path';
import { AgentFactory } from './AgentFactory.js';

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('AgentFactory', () => {
  const agentsDir = path.resolve(import.meta.dirname, '..', '..', '..', 'agents');

  describe('createAllFromDirectory', () => {
    it('should create agents for all manifests in the agents directory', () => {
      const { agents, manifests } = AgentFactory.createAllFromDirectory(agentsDir);

      expect(agents.size).toBe(5);
      expect(manifests.size).toBe(5);

      const names = Array.from(agents.keys()).sort();
      expect(names).toEqual(['architect-prime', 'developer-prime', 'general-assistant', 'researcher-prime', 'writer-prime']);
    });

    it('should return empty maps for a non-existent directory', () => {
      const { agents, manifests } = AgentFactory.createAllFromDirectory('/nonexistent/dir');
      expect(agents.size).toBe(0);
      expect(manifests.size).toBe(0);
    });
  });

  describe('createAgent', () => {
    it('should create an agent from a manifest', () => {
      const { manifests } = AgentFactory.createAllFromDirectory(agentsDir);
      const manifest = manifests.get('architect-prime')!;

      const agent = AgentFactory.createAgent(manifest);
      expect(agent.name).toBe('Winston');
      expect(agent.role).toBe('architect-prime');
    });
  });

  describe('diffAgents', () => {
    it('should detect no changes when directory is unchanged', () => {
      const { manifests } = AgentFactory.createAllFromDirectory(agentsDir);
      const diff = AgentFactory.diffAgents(manifests, agentsDir);

      expect(diff.added).toHaveLength(0);
      expect(diff.removed).toHaveLength(0);
      expect(diff.updated).toHaveLength(0);
    });

    it('should detect all agents as added when starting from empty', () => {
      const empty = new Map();
      const diff = AgentFactory.diffAgents(empty, agentsDir);

      expect(diff.added.length).toBe(5);
      expect(diff.removed).toHaveLength(0);
    });

    it('should detect removed agents', () => {
      const { manifests } = AgentFactory.createAllFromDirectory(agentsDir);
      // Simulate an extra agent that no longer exists on disk
      manifests.set('ghost-agent', {
        apiVersion: 'sera/v1',
        kind: 'Agent',
        metadata: { name: 'ghost-agent', displayName: 'Ghost', icon: '👻', circle: 'test', tier: 1 },
        identity: { role: 'Ghost', description: 'A ghost' },
        model: { provider: 'test', name: 'test' },
      });

      const diff = AgentFactory.diffAgents(manifests, agentsDir);

      expect(diff.removed).toContain('ghost-agent');
    });
  });
});
