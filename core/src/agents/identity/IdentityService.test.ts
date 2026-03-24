import { describe, it, expect } from 'vitest';
import { IdentityService } from './IdentityService.js';
import type { AgentManifest } from '../manifest/types.js';

describe('IdentityService', () => {
  describe('generateSystemPrompt', () => {
    it('should generate a basic prompt with only required metadata', () => {
      const manifest = {
        kind: 'Agent',
        metadata: {
          name: 'test-agent',
          circle: 'test-circle',
        },
      } as unknown as AgentManifest;

      const prompt = IdentityService.generateSystemPrompt(manifest);

      expect(prompt).toContain('You are test-agent.');
      expect(prompt).toContain('## Response Format');
      expect(prompt).toContain('You MUST respond in JSON format');
      expect(prompt).toContain('You belong to the "test-circle" circle.');
    });

    it('should include role and description if provided in identity', () => {
      const manifest = {
        kind: 'Agent',
        metadata: {
          name: 'test-agent',
          circle: 'test-circle',
        },
        identity: {
          role: 'helpful assistant',
          description: 'You assist users with general queries.',
        },
      } as unknown as AgentManifest;

      const prompt = IdentityService.generateSystemPrompt(manifest);

      expect(prompt).toContain('You are test-agent, a helpful assistant.');
      expect(prompt).toContain('You assist users with general queries.');
    });

    it('should use displayName if available', () => {
      const manifest = {
        kind: 'Agent',
        metadata: {
          name: 'test-agent',
          displayName: 'Test Assistant',
          circle: 'test-circle',
        },
      } as unknown as AgentManifest;

      const prompt = IdentityService.generateSystemPrompt(manifest);
      expect(prompt).toContain('You are Test Assistant.');
    });

    it('should include communication style and principles', () => {
      const manifest = {
        kind: 'Agent',
        metadata: {
          name: 'test-agent',
          circle: 'test-circle',
        },
        identity: {
          role: 'expert',
          communicationStyle: 'Professional and concise.',
          principles: ['Be accurate', 'Be polite'],
        },
      } as unknown as AgentManifest;

      const prompt = IdentityService.generateSystemPrompt(manifest);
      expect(prompt).toContain('## Communication Style\nProfessional and concise.');
      expect(prompt).toContain('## Guiding Principles\n- Be accurate\n- Be polite');
    });

    it('should prioritize spec-wrapped format over flat format', () => {
      const manifest = {
        kind: 'Agent',
        metadata: { name: 'test-agent' },
        identity: { role: 'flat-role', principles: ['flat-principle'] },
        spec: {
          identity: { role: 'spec-role', principles: ['spec-principle'] },
        },
      } as unknown as AgentManifest;

      const prompt = IdentityService.generateSystemPrompt(manifest);
      expect(prompt).toContain('You are test-agent, a spec-role.');
      expect(prompt).toContain('## Guiding Principles\n- spec-principle');
      expect(prompt).not.toContain('flat-role');
      expect(prompt).not.toContain('flat-principle');
    });

    it('should include allowed and denied tools', () => {
      const manifest = {
        kind: 'Agent',
        metadata: { name: 'test-agent' },
        tools: {
          allowed: ['tool1', 'tool2'],
          denied: ['tool3'],
        },
      } as unknown as AgentManifest;

      const prompt = IdentityService.generateSystemPrompt(manifest);
      expect(prompt).toContain('## Available Tools\n- tool1\n- tool2');
      expect(prompt).toContain('## Denied Tools (never use these)\n- tool3');
    });

    it('should include allowed subagents', () => {
      const manifest = {
        kind: 'Agent',
        metadata: { name: 'test-agent' },
        subagents: {
          allowed: [
            { role: 'researcher', maxInstances: 2 },
            { role: 'writer', requiresApproval: true },
          ],
        },
      } as unknown as AgentManifest;

      const prompt = IdentityService.generateSystemPrompt(manifest);
      expect(prompt).toContain(
        '## Subagents You Can Spawn\n- researcher (max 2 instances)\n- writer (max ∞ instances) [requires human approval]'
      );
    });

    it('should include skills', () => {
      const manifest = {
        kind: 'Agent',
        metadata: { name: 'test-agent' },
        skills: ['skill1', 'skill2'],
      } as unknown as AgentManifest;

      const prompt = IdentityService.generateSystemPrompt(manifest);
      expect(prompt).toContain('## Available Skills\n- skill1\n- skill2');
    });

    it('should append circle context and dynamic memory context', () => {
      const manifest = {
        kind: 'Agent',
        metadata: { name: 'test-agent' },
      } as unknown as AgentManifest;

      const prompt = IdentityService.generateSystemPrompt(
        manifest,
        'Important project info.',
        'Past interactions memory.'
      );
      expect(prompt).toContain(
        '## Project Context\nThe following project context is shared by all agents in your circle:\n\nImportant project info.'
      );
      expect(prompt).toContain('Past interactions memory.');
    });
  });

  describe('generateStreamingSystemPrompt', () => {
    it('should modify response format to natural language', () => {
      const manifest = {
        kind: 'Agent',
        metadata: {
          name: 'test-agent',
        },
      } as unknown as AgentManifest;

      const streamingPrompt = IdentityService.generateStreamingSystemPrompt(manifest);

      expect(streamingPrompt).toContain(
        '## Response Format\nRespond directly and naturally in markdown. Do NOT wrap your response in JSON.'
      );
      expect(streamingPrompt).not.toContain('You MUST respond in JSON format');
    });

    it('should append stability guidelines', () => {
      const manifest = {
        kind: 'Agent',
        metadata: { name: 'test-agent' },
      } as unknown as AgentManifest;

      const streamingPrompt = IdentityService.generateStreamingSystemPrompt(manifest);
      expect(streamingPrompt).toContain('## Stability Guidelines');
      expect(streamingPrompt).toContain(
        '- Do NOT call the same tool with the same arguments repeatedly.'
      );
    });

    it('should preserve regular sections like tools or identity', () => {
      const manifest = {
        kind: 'Agent',
        metadata: { name: 'test-agent' },
        identity: { role: 'tester' },
        tools: { allowed: ['test-tool'] },
      } as unknown as AgentManifest;

      const streamingPrompt = IdentityService.generateStreamingSystemPrompt(manifest);
      expect(streamingPrompt).toContain('You are test-agent, a tester.');
      expect(streamingPrompt).toContain('## Available Tools\n- test-tool');
    });
  });
});
