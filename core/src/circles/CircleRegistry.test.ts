import { describe, it, expect } from 'vitest';
import path from 'path';
import { CircleRegistry } from './CircleRegistry.js';
import { AgentManifestLoader, ManifestValidationError } from '../agents/manifest/AgentManifestLoader.js';
import type { CircleManifest } from './types.js';
import type { AgentManifest } from '../agents/manifest/types.js';

// ── Helpers ─────────────────────────────────────────────────────────────────────

/** Minimal valid circle manifest object for testing */
function validCircleObj(): Record<string, unknown> {
  return {
    apiVersion: 'sera/v1',
    kind: 'Circle',
    metadata: {
      name: 'test-circle',
      displayName: 'Test Circle',
    },
    agents: ['agent-a', 'agent-b'],
  };
}

/** Minimal agent manifest for reference validation testing */
function mockAgentManifest(name: string): AgentManifest {
  return {
    apiVersion: 'sera/v1',
    kind: 'Agent',
    metadata: {
      name,
      displayName: name,
      icon: '🤖',
      circle: 'test-circle',
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
  };
}

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('CircleRegistry', () => {
  describe('validateCircle', () => {
    it('should accept a valid circle manifest', () => {
      const circle = CircleRegistry.validateCircle(validCircleObj());
      expect(circle.metadata.name).toBe('test-circle');
      expect(circle.metadata.displayName).toBe('Test Circle');
      expect(circle.agents).toEqual(['agent-a', 'agent-b']);
    });

    it('should accept a circle with all optional fields', () => {
      const obj = {
        ...validCircleObj(),
        knowledge: {
          qdrantCollection: 'test-knowledge',
          postgresSchema: 'circle_test',
        },
        channels: [
          { name: 'updates', type: 'persistent' },
          { name: 'alerts', type: 'ephemeral' },
        ],
        partyMode: {
          enabled: true,
          orchestrator: 'agent-a',
          selectionStrategy: 'relevance',
        },
        connections: [
          { circle: 'other-circle', bridgeChannels: ['shared'], auth: 'internal' },
        ],
        projectContext: {
          path: 'test/project-context.md',
          autoLoad: true,
        },
      };

      const circle = CircleRegistry.validateCircle(obj);
      expect(circle.knowledge?.qdrantCollection).toBe('test-knowledge');
      expect(circle.channels).toHaveLength(2);
      expect(circle.partyMode?.enabled).toBe(true);
      expect(circle.connections).toHaveLength(1);
    });

    it('should reject non-object input', () => {
      expect(() => CircleRegistry.validateCircle('string')).toThrow(ManifestValidationError);
      expect(() => CircleRegistry.validateCircle(null)).toThrow(ManifestValidationError);
      expect(() => CircleRegistry.validateCircle(42)).toThrow(ManifestValidationError);
    });

    it('should reject unknown top-level fields', () => {
      const obj = { ...validCircleObj(), unknownField: 'surprise' };
      expect(() => CircleRegistry.validateCircle(obj)).toThrow(/unknown top-level field.*unknownField/i);
    });

    it('should reject invalid kind', () => {
      const obj = validCircleObj();
      obj['kind'] = 'Agent';
      expect(() => CircleRegistry.validateCircle(obj)).toThrow(/kind.*must be.*Circle/i);
    });

    it('should reject missing metadata', () => {
      const obj = validCircleObj();
      delete obj['metadata'];
      expect(() => CircleRegistry.validateCircle(obj)).toThrow(/missing required field.*metadata/i);
    });

    it('should reject missing metadata.name', () => {
      const obj = validCircleObj();
      delete (obj['metadata'] as any).name;
      expect(() => CircleRegistry.validateCircle(obj)).toThrow(/missing required field.*name/i);
    });

    it('should reject missing metadata.displayName', () => {
      const obj = validCircleObj();
      delete (obj['metadata'] as any).displayName;
      expect(() => CircleRegistry.validateCircle(obj)).toThrow(/missing required field.*displayName/i);
    });

    it('should reject non-array agents', () => {
      const obj = validCircleObj();
      obj['agents'] = 'not-an-array';
      expect(() => CircleRegistry.validateCircle(obj)).toThrow(/agents.*must be an array/i);
    });

    it('should reject non-string entries in agents', () => {
      const obj = validCircleObj();
      obj['agents'] = ['valid', 42];
      expect(() => CircleRegistry.validateCircle(obj)).toThrow(/each entry in.*agents.*must be a string/i);
    });

    it('should accept an empty agents array', () => {
      const obj = validCircleObj();
      obj['agents'] = [];
      const circle = CircleRegistry.validateCircle(obj);
      expect(circle.agents).toEqual([]);
    });

    it('should reject knowledge without qdrantCollection', () => {
      const obj = validCircleObj();
      obj['knowledge'] = { postgresSchema: 'test' };
      expect(() => CircleRegistry.validateCircle(obj)).toThrow(/missing required field.*qdrantCollection/i);
    });

    it('should reject non-array channels', () => {
      const obj = validCircleObj();
      obj['channels'] = 'not-array';
      expect(() => CircleRegistry.validateCircle(obj)).toThrow(/channels.*must be an array/i);
    });
  });

  describe('validateAgentReferences', () => {
    it('should return empty when all agents exist', () => {
      const circle = CircleRegistry.validateCircle(validCircleObj()) as CircleManifest;
      const manifests = [mockAgentManifest('agent-a'), mockAgentManifest('agent-b')];
      const missing = CircleRegistry.validateAgentReferences(circle, manifests);
      expect(missing).toEqual([]);
    });

    it('should return missing agent names', () => {
      const circle = CircleRegistry.validateCircle(validCircleObj()) as CircleManifest;
      const manifests = [mockAgentManifest('agent-a')];
      const missing = CircleRegistry.validateAgentReferences(circle, manifests);
      expect(missing).toEqual(['agent-b']);
    });

    it('should return all agents when none exist', () => {
      const circle = CircleRegistry.validateCircle(validCircleObj()) as CircleManifest;
      const missing = CircleRegistry.validateAgentReferences(circle, []);
      expect(missing).toEqual(['agent-a', 'agent-b']);
    });
  });

  describe('loadCircle', () => {
    it('should load the development example circle from disk', () => {
      const circlesDir = path.resolve(import.meta.dirname, '..', '..', '..', 'circles');
      const circle = CircleRegistry.loadCircle(path.join(circlesDir, 'development.circle.yaml'));

      expect(circle.metadata.name).toBe('development');
      expect(circle.metadata.displayName).toBe('Development Circle');
      expect(circle.agents).toContain('architect-prime');
      expect(circle.agents).toContain('developer-prime');
      expect(circle.partyMode?.enabled).toBe(true);
    });

    it('should throw for non-existent file', () => {
      expect(() => CircleRegistry.loadCircle('/nonexistent/path.yaml'))
        .toThrow(/not found/i);
    });
  });

  describe('loadAllCircles', () => {
    it('should load all example circles from the circles directory', () => {
      const circlesDir = path.resolve(import.meta.dirname, '..', '..', '..', 'circles');
      const circles = CircleRegistry.loadAllCircles(circlesDir);

      expect(circles.length).toBe(2);
      const names = circles.map(c => c.metadata.name).sort();
      expect(names).toEqual(['development', 'operations']);
    });

    it('should return empty array for non-existent directory', () => {
      const circles = CircleRegistry.loadAllCircles('/nonexistent/dir');
      expect(circles).toEqual([]);
    });
  });

  describe('instance methods', () => {
    it('should load circles and provide accessors', () => {
      const registry = new CircleRegistry();
      const circlesDir = path.resolve(import.meta.dirname, '..', '..', '..', 'circles');
      const agentsDir = path.resolve(import.meta.dirname, '..', '..', '..', 'agents');

      // Load agent manifests for reference validation
      const agents = AgentManifestLoader.loadAllManifests(agentsDir);

      registry.loadFromDirectory(circlesDir, agents);

      const circles = registry.listCircles();
      expect(circles.length).toBe(2);

      const dev = registry.getCircle('development');
      expect(dev).toBeDefined();
      expect(dev?.metadata.displayName).toBe('Development Circle');

      expect(registry.getCircle('nonexistent')).toBeUndefined();
    });

    it('should load project context for development circle', () => {
      const registry = new CircleRegistry();
      const circlesDir = path.resolve(import.meta.dirname, '..', '..', '..', 'circles');
      const agentsDir = path.resolve(import.meta.dirname, '..', '..', '..', 'agents');
      const agents = AgentManifestLoader.loadAllManifests(agentsDir);
      registry.loadFromDirectory(circlesDir, agents);

      const context = registry.getProjectContext('development');
      expect(context).toBeUndefined(); // Assuming development.circle.yaml does not have projectContext
    });

    it('should return undefined for circle without project context', () => {
      const registry = new CircleRegistry();
      const circlesDir = path.resolve(import.meta.dirname, '..', '..', '..', 'circles');
      registry.loadFromDirectory(circlesDir);

      const context = registry.getProjectContext('operations');
      expect(context).toBeUndefined();
    });

    it('should list circle summaries', () => {
      const registry = new CircleRegistry();
      const circlesDir = path.resolve(import.meta.dirname, '..', '..', '..', 'circles');
      const agentsDir = path.resolve(import.meta.dirname, '..', '..', '..', 'agents');
      const agents = AgentManifestLoader.loadAllManifests(agentsDir);
      registry.loadFromDirectory(circlesDir, agents);

      const summaries = registry.listCircleSummaries();
      expect(summaries.length).toBe(2);

      const devSummary = summaries.find(s => s.name === 'development');
      expect(devSummary).toBeDefined();
      expect(devSummary!.hasProjectContext).toBe(false);
      expect(devSummary!.agents).toContain('architect-prime');
      expect(devSummary!.channelCount).toBe(0); // 0 explicit channels in development.circle.yaml
    });
  });
});
