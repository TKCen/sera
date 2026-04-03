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

    it('includes all sections when fully configured', () => {
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
          principles: ['Always honest'],
          communicationStyle: 'Brief',
          notes: 'Agent notes here.',
        },
        model: {
          provider: 'openai',
          name: 'gpt-4',
        },
        contextFiles: [{ path: 'README.md', label: 'README' }],
        outputFormat: 'Markdown',
      };

      const prompt = generateSystemPrompt(manifest, {
        tools: [
          {
            type: 'function',
            function: {
              name: 'test-tool',
              description: 'A test tool.',
              parameters: { type: 'object', properties: {} },
            },
          },
        ],
        circleName: 'Engineering',
        circleMembers: ['alice', 'bob'],
        availableAgents: [{ name: 'sub-agent', role: 'Support' }],
      });

      expect(prompt).toContain('You are Test Agent, a SERA AI agent.');
      expect(prompt).toContain('## Principles');
      expect(prompt).toContain('Always honest');
      expect(prompt).toContain('## Communication Style');
      expect(prompt).toContain('Brief');
      expect(prompt).toContain('## Available Tools');
      expect(prompt).toContain('test-tool');
      expect(prompt).toContain('## System Context');
      expect(prompt).toContain('## Circle: Engineering');
      expect(prompt).toContain('## Delegation');
      expect(prompt).toContain('## Agent Notes');
      expect(prompt).toContain('Agent notes here.');
      expect(prompt).toContain('## Workspace Context');
      expect(prompt).toContain('### README');
      expect(prompt).toContain('## System Constraints');
      expect(prompt).toContain('## Output Format');
    });

    it('respects the token budget', () => {
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
          principles: [
            'A very long principle that will definitely add many tokens to the prompt to make sure budget is exceeded',
          ],
        },
        model: {
          provider: 'openai',
          name: 'gpt-4',
        },
      };

      // Set a tiny budget to force dropping optional sections
      const prompt = generateSystemPrompt(manifest, { tokenBudget: 20 });
      expect(prompt).toContain('You are Test Agent');
      // identity (required) should be there, but principles (optional) should be dropped
      expect(prompt).not.toContain('## Principles');
    });
  });
});
