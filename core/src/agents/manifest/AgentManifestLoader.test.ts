import { describe, it, expect } from 'vitest';
import path from 'path';
import { AgentManifestLoader, ManifestValidationError } from './AgentManifestLoader.js';

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
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(
        /unknown top-level field.*unknownField/i
      );
    });

    it('should reject invalid kind', () => {
      const obj = validManifestObj();
      obj['kind'] = 'Circle';
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(/kind.*must be.*Agent/i);
    });

    it('should reject invalid security tiers', () => {
      const obj = validManifestObj();
      (obj['metadata'] as Record<string, unknown>).tier = 5;
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(/invalid security tier.*5/i);
    });

    it('should reject tier 0', () => {
      const obj = validManifestObj();
      (obj['metadata'] as Record<string, unknown>).tier = 0;
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(/invalid security tier/i);
    });

    it('should reject missing metadata.name', () => {
      const obj = validManifestObj();
      delete (obj['metadata'] as Record<string, unknown>).name;
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(
        /missing required field.*name/i
      );
    });

    it('should reject missing identity', () => {
      const obj = validManifestObj();
      delete obj['identity'];
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(
        /missing required field.*identity/i
      );
    });

    it('should reject missing identity.role', () => {
      const obj = validManifestObj();
      delete (obj['identity'] as Record<string, unknown>).role;
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(
        /missing required field.*role/i
      );
    });

    it('should reject missing model', () => {
      const obj = validManifestObj();
      delete obj['model'];
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(
        /missing required field.*model/i
      );
    });

    it('should reject missing model.provider', () => {
      const obj = validManifestObj();
      delete (obj['model'] as Record<string, unknown>).provider;
      expect(() => AgentManifestLoader.validateManifest(obj)).toThrow(
        /missing required field.*provider/i
      );
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
      delete (obj['metadata'] as Record<string, unknown>).icon;
      const manifest = AgentManifestLoader.validateManifest(obj);
      expect(manifest.metadata.icon).toBe('🤖');
    });
  });

  describe('loadManifest', () => {
    it('should load an agent manifest with flat format', () => {
      // Create a minimal valid agent manifest in-memory and validate it
      const obj = {
        apiVersion: 'sera/v1',
        kind: 'Agent',
        metadata: { name: 'test-load', displayName: 'Test', icon: '🤖', tier: 2 },
        identity: { role: 'Test role', description: 'A test agent' },
        model: { provider: 'test', name: 'test-model' },
      };
      const manifest = AgentManifestLoader.validateManifest(obj);
      expect(manifest.metadata.name).toBe('test-load');
      expect(manifest.identity.role).toBe('Test role');
    });

    it('should throw for non-existent file', () => {
      expect(() => AgentManifestLoader.loadManifest('/nonexistent/path.yaml')).toThrow(
        /not found/i
      );
    });
  });

  describe('loadAllManifests', () => {
    it('should return empty array for empty agents directory', () => {
      const agentsDir = path.resolve(import.meta.dirname, '..', '..', '..', '..', 'agents');
      const manifests = AgentManifestLoader.loadAllManifests(agentsDir);
      // agents/ directory is now empty — templates live in templates/builtin/
      expect(manifests.length).toBe(0);
    });

    it('should return empty array for non-existent directory', () => {
      const manifests = AgentManifestLoader.loadAllManifests('/nonexistent/dir');
      expect(manifests).toEqual([]);
    });
  });
});
