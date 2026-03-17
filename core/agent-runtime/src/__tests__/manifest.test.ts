import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import fs from 'fs';
import path from 'path';
import os from 'os';
import { loadManifest, generateSystemPrompt, type RuntimeManifest } from '../manifest.js';

describe('manifest', () => {
  let tempDir: string;
  let validManifestPath: string;
  let missingNameManifestPath: string;

  beforeEach(() => {
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sera-manifest-test-'));
    validManifestPath = path.join(tempDir, 'valid-AGENT.yaml');
    missingNameManifestPath = path.join(tempDir, 'invalid-AGENT.yaml');

    const validYaml = `
apiVersion: v1
kind: Agent
metadata:
  name: test-agent
  displayName: Test Agent
  icon: robot
  circle: default
  tier: 1
identity:
  role: Helpful Assistant
  description: A test agent.
model:
  provider: openai
  name: gpt-4
`;
    fs.writeFileSync(validManifestPath, validYaml, 'utf-8');

    const invalidYaml = `
apiVersion: v1
kind: Agent
metadata:
  displayName: No Name Agent
identity:
  role: Helpful Assistant
  description: A test agent.
model:
  provider: openai
  name: gpt-4
`;
    fs.writeFileSync(missingNameManifestPath, invalidYaml, 'utf-8');
  });

  afterEach(() => {
    fs.rmSync(tempDir, { recursive: true, force: true });
  });

  describe('loadManifest()', () => {
    it('parses a valid AGENT.yaml and returns a typed RuntimeManifest', () => {
      const manifest = loadManifest(validManifestPath);
      expect(manifest.metadata.name).toBe('test-agent');
      expect(manifest.metadata.displayName).toBe('Test Agent');
      expect(manifest.identity.role).toBe('Helpful Assistant');
    });

    it('throws on missing file', () => {
      const missingPath = path.join(tempDir, 'does-not-exist.yaml');
      expect(() => loadManifest(missingPath)).toThrow(/Manifest not found/);
    });

    it('throws on manifest with missing metadata.name', () => {
      expect(() => loadManifest(missingNameManifestPath)).toThrow(/missing metadata.name/);
    });
  });

  describe('generateSystemPrompt()', () => {
    it("includes the agent's role and description", () => {
      const manifest: RuntimeManifest = {
        apiVersion: 'v1',
        kind: 'Agent',
        metadata: {
          name: 'test-agent',
          displayName: 'Test Agent',
          icon: 'robot',
          circle: 'default',
          tier: 1,
        },
        identity: {
          role: 'Helpful Assistant',
          description: 'A test agent.',
        },
        model: {
          provider: 'openai',
          name: 'gpt-4',
        },
      };

      const prompt = generateSystemPrompt(manifest);
      expect(prompt).toContain('Role: Helpful Assistant');
      expect(prompt).toContain('Description: A test agent.');
    });
  });
});
