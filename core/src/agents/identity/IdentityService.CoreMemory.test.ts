import { describe, it, expect } from 'vitest';
import { IdentityService } from './IdentityService.js';
import type { AgentManifest } from '../manifest/types.js';

describe('IdentityService Core Memory', () => {
  const mockManifest: AgentManifest = {
    apiVersion: 'sera/v1',
    kind: 'Agent',
    metadata: { name: 'test-agent', displayName: 'Test Agent', tier: 1, icon: '🤖' },
    identity: { role: 'tester', description: 'test description' },
    model: { provider: 'test', name: 'test-model' },
  };

  it('should format core memory blocks as XML', () => {
    const blocks = [
      { name: 'persona', content: 'I am a helpful assistant.', charLimit: 2000, isReadonly: false },
      { name: 'human', content: 'User likes coffee.', charLimit: 2000, isReadonly: false },
    ];

    const prompt = IdentityService.generateSystemPrompt(mockManifest, undefined, undefined, blocks);

    expect(prompt).toContain('<memory_blocks>');
    expect(prompt).toContain('<persona chars_current="25" chars_limit="2000">');
    expect(prompt).toContain('I am a helpful assistant.');
    expect(prompt).toContain('<human chars_current="18" chars_limit="2000">');
    expect(prompt).toContain('User likes coffee.');
    expect(prompt).toContain('</memory_blocks>');
  });

  it('should include readonly attribute when block is readonly', () => {
    const blocks = [
      { name: 'context', content: 'Fixed context.', charLimit: 1000, isReadonly: true },
    ];

    const prompt = IdentityService.generateSystemPrompt(mockManifest, undefined, undefined, blocks);

    expect(prompt).toContain('<context chars_current="14" chars_limit="1000" readonly="true">');
  });

  it('should handle empty blocks with (empty) placeholder', () => {
    const blocks = [{ name: 'persona', content: '', charLimit: 2000, isReadonly: false }];

    const prompt = IdentityService.generateSystemPrompt(mockManifest, undefined, undefined, blocks);

    expect(prompt).toContain('(empty)');
  });
});
