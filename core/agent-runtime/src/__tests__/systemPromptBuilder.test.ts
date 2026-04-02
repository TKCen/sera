import { describe, it, expect } from 'vitest';
import { SystemPromptBuilder } from '../systemPromptBuilder.js';
import type { RuntimeManifest } from '../manifest.js';
import type { ToolDefinition } from '../llmClient.js';

describe('SystemPromptBuilder', () => {
  const mockManifest: RuntimeManifest = {
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
      role: 'Test Assistant',
      description: 'A test agent for unit tests.',
      principles: ['Be helpful', 'Be concise'],
      communicationStyle: 'Professional',
      notes: 'Some internal notes.',
    },
    model: {
      provider: 'openai',
      name: 'gpt-4',
    },
    contextFiles: [{ path: 'README.md', label: 'Readme' }],
    outputFormat: 'Markdown',
  };

  const mockTools: ToolDefinition[] = [
    {
      type: 'function',
      function: {
        name: 'test-tool',
        description: 'A test tool description.',
        parameters: { type: 'object', properties: {} },
      },
    },
  ];

  it('builds a full system prompt with all sections', () => {
    const builder = new SystemPromptBuilder();
    builder
      .addIdentity(mockManifest)
      .addPrinciples(mockManifest)
      .addCommunicationStyle(mockManifest)
      .addAvailableTools(mockTools)
      .addToolUsageGuidelines()
      .addMemoryInstructions()
      .addTimeContext()
      .addCircleContext('default', ['user1'])
      .addDelegationContext([{ name: 'sub-agent', role: 'Sub-agent role' }])
      .addAgentNotes(mockManifest)
      .addWorkspaceContext(mockManifest)
      .addReasoningHints('gpt-4-thinking')
      .addConstraints(1)
      .addOutputFormat(mockManifest.outputFormat);

    const prompt = builder.build();

    expect(prompt).toContain('You are Test Agent, a SERA AI agent.');
    expect(prompt).toContain('Role: Test Assistant');
    expect(prompt).toContain('## Principles');
    expect(prompt).toContain('- Be helpful');
    expect(prompt).toContain('## Communication Style');
    expect(prompt).toContain('Professional');
    expect(prompt).toContain('## Available Tools');
    expect(prompt).toContain('- **test-tool**: A test tool description.');
    expect(prompt).toContain('## Tool Usage Guidelines');
    expect(prompt).toContain('## Memory & Knowledge');
    expect(prompt).toContain('## System Context');
    expect(prompt).toContain('## Circle: default');
    expect(prompt).toContain('## Delegation');
    expect(prompt).toContain('## Agent Notes');
    expect(prompt).toContain('Some internal notes.');
    expect(prompt).toContain('## Workspace Context');
    expect(prompt).toContain('### Readme');
    expect(prompt).toContain('## Reasoning Instructions');
    expect(prompt).toContain('## System Constraints');
    expect(prompt).toContain('## Output Format');
    expect(prompt).toContain('Markdown');
  });

  it('respects token budget by dropping optional sections', () => {
    const builder = new SystemPromptBuilder();
    // Required: identity (approx 20 tokens), constraints (approx 15 tokens)
    // Optional: principles (approx 15 tokens)
    builder
      .addIdentity(mockManifest)
      .addPrinciples(mockManifest)
      .addConstraints(1);

    // Total should be around 50 tokens. Set budget to 35.
    // Principles (priority 10) should be dropped. Identity and Constraints are required.
    const prompt = builder.build(35);

    expect(prompt).toContain('You are Test Agent');
    expect(prompt).toContain('## System Constraints');
    expect(prompt).not.toContain('## Principles');
  });

  it('keeps all required sections even if over budget', () => {
    const builder = new SystemPromptBuilder();
    builder
      .addIdentity(mockManifest)
      .addConstraints(1);

    // Total approx 35 tokens. Set budget to 10.
    const prompt = builder.build(10);

    expect(prompt).toContain('You are Test Agent');
    expect(prompt).toContain('## System Constraints');
  });

  it('handles empty optional fields gracefully', () => {
    const minimalManifest: RuntimeManifest = {
      apiVersion: 'v1',
      kind: 'Agent',
      metadata: {
        name: 'minimal',
        displayName: 'Minimal',
        icon: 'robot',
        circle: 'default',
        tier: 1,
      },
      identity: {
        role: 'Minimalist',
        description: 'Less is more.',
      },
      model: {
        provider: 'openai',
        name: 'gpt-4',
      },
    };

    const builder = new SystemPromptBuilder();
    builder
      .addIdentity(minimalManifest)
      .addPrinciples(minimalManifest)
      .addAgentNotes(minimalManifest)
      .addWorkspaceContext(minimalManifest);

    const prompt = builder.build();
    expect(prompt).toContain('You are Minimal, a SERA AI agent.');
    expect(prompt).not.toContain('## Principles');
    expect(prompt).not.toContain('## Agent Notes');
    expect(prompt).not.toContain('## Workspace Context');
  });
});
