import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { SystemPromptBuilder } from '../systemPromptBuilder.js';
import type { RuntimeManifest } from '../manifest.js';
import type { ToolDefinition } from '../llmClient.js';
import fs from 'fs';

vi.mock('fs');

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
    contextFiles: [{ path: 'README.md', label: 'README' }],
    outputFormat: 'Markdown',
    notes: 'Agent-specific notes here.',
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

  beforeEach(() => {
    vi.mocked(fs.readFileSync).mockImplementation(() => 'Mocked file content');
    vi.mocked(fs.existsSync).mockImplementation(() => true);
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

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
    expect(prompt).toContain('Agent-specific notes here.');
    expect(prompt).toContain('## Workspace Context');
    expect(prompt).toContain('### README');
    expect(prompt).toContain('Mocked file content');
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

  describe('Workspace Context Trimming', () => {
    beforeEach(() => {
      vi.mocked(fs.existsSync).mockImplementation(() => true);
      process.env['CONTEXT_FILES_BUDGET'] = '100'; // Very small budget for testing
    });

    afterEach(() => {
      delete process.env['CONTEXT_FILES_BUDGET'];
    });

    it('trims low priority files first', () => {
      process.env['CONTEXT_FILES_BUDGET'] = '150';
      const manifest: RuntimeManifest = {
        ...mockManifest,
        contextFiles: [
          { path: 'high.md', label: 'High', priority: 'high' },
          { path: 'low.md', label: 'Low', priority: 'low' },
        ],
      };

      vi.mocked(fs.readFileSync).mockImplementation((path) => {
        if (path.toString().endsWith('high.md')) return 'A'.repeat(200); // ~50 tokens
        if (path.toString().endsWith('low.md')) return 'B'.repeat(800); // ~200 tokens
        return '';
      });

      const builder = new SystemPromptBuilder();
      builder.addWorkspaceContext(manifest);
      const prompt = builder.build();

      expect(prompt).toContain('### High');
      expect(prompt).toContain('A'.repeat(200));
      expect(prompt).toContain('### Low');
      expect(prompt).toContain('*Omitted due to token budget: low.md*');
    });

    it('trims largest files first within same priority', () => {
      const manifest: RuntimeManifest = {
        ...mockManifest,
        contextFiles: [
          { path: 'small.md', label: 'Small', priority: 'normal' },
          { path: 'large.md', label: 'Large', priority: 'normal' },
        ],
      };

      vi.mocked(fs.readFileSync).mockImplementation((path) => {
        if (path.toString().endsWith('small.md')) return 'S'.repeat(40); // ~10 tokens
        if (path.toString().endsWith('large.md')) return 'L'.repeat(400); // ~100 tokens
        return '';
      });

      const builder = new SystemPromptBuilder();
      builder.addWorkspaceContext(manifest);
      const prompt = builder.build();

      expect(prompt).toContain('### Small');
      expect(prompt).toContain('S'.repeat(40));
      expect(prompt).toContain('### Large');
      expect(prompt).toContain('*Omitted due to token budget: large.md*');
    });

    it('truncates high priority files instead of omitting', () => {
      const manifest: RuntimeManifest = {
        ...mockManifest,
        contextFiles: [{ path: 'high.md', label: 'High', priority: 'high' }],
      };

      vi.mocked(fs.readFileSync).mockReturnValue('H'.repeat(800)); // ~200 tokens

      const builder = new SystemPromptBuilder();
      builder.addWorkspaceContext(manifest);
      const prompt = builder.build();

      expect(prompt).toContain('### High');
      expect(prompt).toContain('H'.repeat(100)); // Truncated to around budget
      expect(prompt).toContain('...(truncated)');
      expect(prompt).not.toContain('*Omitted due to token budget*');
    });

    it('handles missing files gracefully', () => {
      vi.mocked(fs.existsSync).mockImplementation(() => false);
      vi.mocked(fs.readFileSync).mockImplementation(() => {
        throw new Error('File not found');
      });
      const manifest: RuntimeManifest = {
        ...mockManifest,
        contextFiles: [{ path: 'missing.md', label: 'Missing' }],
      };

      const builder = new SystemPromptBuilder();
      builder.addWorkspaceContext(manifest);
      const prompt = builder.build();

      expect(prompt).toContain('### Missing');
      expect(prompt).toContain('*File not found: missing.md*');
    });

    it('blocks path traversal', () => {
      const manifest: RuntimeManifest = {
        ...mockManifest,
        contextFiles: [{ path: '../etc/passwd', label: 'Secret' }],
      };

      const builder = new SystemPromptBuilder();
      builder.addWorkspaceContext(manifest);
      const prompt = builder.build();

      expect(prompt).toContain('### Secret');
      expect(prompt).toContain('*Path traversal blocked: ../etc/passwd*');
    });
  });
});
