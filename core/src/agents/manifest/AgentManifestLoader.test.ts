import { describe, it, expect } from 'vitest';
import path from 'path';
import { AgentManifestLoader, ManifestValidationError } from './AgentManifestLoader.js';
import type { AgentManifest } from './types.js';

// ── Helpers ─────────────────────────────────────────────────────────────────────

/** Minimal valid manifest object for testing */
function validManifestObj(): Record<string, unknown> {
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
  };
}

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('AgentManifestLoader', () => {
  describe('validateManifest', () => {
    it('should accept a valid manifest', () => {
      const manifest = AgentManifestLoader.validateManifest(validManifestObj());
      expect(manifest.metadata.name).toBe('test-agent');
      expect(manifest.metadata.tier).toBe(2);
      expect(manifest.identity.role).toBe('Tester');
      expect(manifest.model.provider).toBe('lm-studio');
    });

    it('should reject non-object input', () => {
      expect(() => AgentManifestLoader.validateManifest('string')).toThrow(ManifestValidationError);
      expect(() => AgentManifestLoader.validateManifest(null)).toThrow(ManifestValidationError);
      expect(() => AgentManifestLoader.validateManifest(42)).toThrow(ManifestValidationError);
    });

    it('should reject unknown top-level fields', () => {
      const obj = { ...validManifestObj(), unknownField: 'surprise' };
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(/unknown top-level field.*unknownField/i);
    });

    it('should reject invalid kind', () => {
      const obj = validManifestObj();
      obj['kind'] = 'Circle';
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(/kind.*must be.*Agent/i);
    });

    it('should reject invalid security tiers', () => {
      const obj = validManifestObj();
      (obj['metadata'] as any).tier = 5;
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(/invalid security tier.*5/i);
    });

    it('should reject tier 0', () => {
      const obj = validManifestObj();
      (obj['metadata'] as any).tier = 0;
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(/invalid security tier/i);
    });

    it('should reject missing metadata.name', () => {
      const obj = validManifestObj();
      delete (obj['metadata'] as any).name;
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(/missing required field.*name/i);
    });

    it('should reject missing identity', () => {
      const obj = validManifestObj();
      delete obj['identity'];
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(/missing required field.*identity/i);
    });

    it('should reject missing identity.role', () => {
      const obj = validManifestObj();
      delete (obj['identity'] as any).role;
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(/missing required field.*role/i);
    });

    it('should reject missing model', () => {
      const obj = validManifestObj();
      delete obj['model'];
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(/missing required field.*model/i);
    });

    it('should reject missing model.provider', () => {
      const obj = validManifestObj();
      delete (obj['model'] as any).provider;
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(/missing required field.*provider/i);
    });

    it('should accept manifest without optional fields', () => {
      const obj = validManifestObj();
      // No tools, skills, subagents, intercom, resources, workspace, memory
      const manifest = AgentManifestLoader.validateManifest(obj);
      expect(manifest.tools).toBeUndefined();
      expect(manifest.skills).toBeUndefined();
      expect(manifest.subagents).toBeUndefined();
    });

    it('should default icon to 🤖 when not provided', () => {
      const obj = validManifestObj();
      delete (obj['metadata'] as any).icon;
      const manifest = AgentManifestLoader.validateManifest(obj);
      expect(manifest.metadata.icon).toBe('🤖');
    });
  });

  describe('loadManifest', () => {
    it('should load the architect example manifest from disk', () => {
      const agentsDir = path.resolve(import.meta.dirname, '..', '..', '..', '..', 'agents');
      const manifest = AgentManifestLoader.loadManifest(path.join(agentsDir, 'architect.agent.yaml'));

      expect(manifest.metadata.name).toBe('architect-prime');
      expect(manifest.metadata.displayName).toBe('Winston');
      expect(manifest.metadata.tier).toBe(2);
      expect(manifest.identity.role).toContain('Architect');
      expect(manifest.model.provider).toBe('lm-studio');
      expect(manifest.tools?.allowed).toContain('file-read');
      expect(manifest.skills).toContain('create-architecture');
    });

    it('should throw for non-existent file', () => {
      expect(() => AgentManifestLoader.loadManifest('/nonexistent/path.yaml'))
        .toThrow(/not found/i);
    });
  });

  describe('loadAllManifests', () => {
    it('should load all example manifests from the agents directory', () => {
      const agentsDir = path.resolve(import.meta.dirname, '..', '..', '..', '..', 'agents');
      const manifests = AgentManifestLoader.loadAllManifests(agentsDir);

      expect(manifests.length).toBe(5);
      const names = manifests.map(m => m.metadata.name).sort();
      expect(names).toEqual(['architect-prime', 'developer-prime', 'general-assistant', 'researcher-prime', 'writer']);
    });

    it('should return empty array for non-existent directory', () => {
      const manifests = AgentManifestLoader.loadAllManifests('/nonexistent/dir');
      expect(manifests).toEqual([]);
    });
  });
});
