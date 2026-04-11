import { describe, it, expect, vi, beforeEach } from 'vitest';
import { MemoryAnalyst } from './MemoryAnalyst.js';
import { LlmRouter } from '../llm/LlmRouter.js';

describe('MemoryAnalyst', () => {
  let llmRouter: LlmRouter;
  let analyst: MemoryAnalyst;

  beforeEach(() => {
    llmRouter = {
      chatCompletion: vi.fn(),
    } as unknown as LlmRouter;
    analyst = new MemoryAnalyst(llmRouter);
  });

  it('should analyze content and return atomic facts', async () => {
    const mockContent = 'The project deadline is April 2026. The main contact is Alice.';
    const mockLlmResponse = {
      response: {
        choices: [
          {
            message: {
              content: JSON.stringify({
                facts: [
                  {
                    title: 'Project Deadline',
                    content: 'The project deadline is April 2026.',
                    importance: 4,
                    tags: ['deadline', 'project'],
                    scope: 'circle',
                  },
                  {
                    title: 'Main Contact',
                    content: 'The main contact is Alice.',
                    importance: 3,
                    tags: ['contact', 'alice'],
                    scope: 'personal',
                  },
                ],
              }),
            },
          },
        ],
      },
    };

    (llmRouter.chatCompletion as any) = vi.fn().mockResolvedValue(mockLlmResponse as any);

    const result = await analyst.analyze(mockContent, 'test-model');

    expect(result.facts).toHaveLength(2);
    expect(result.facts[0]!.title).toBe('Project Deadline');
    expect(result.facts[1]!.title).toBe('Main Contact');
    expect(result.facts[0]!.importance).toBe(4);
    expect(result.facts[1]!.scope).toBe('personal');
  });

  it('should handle malformed JSON from LLM by returning fallback', async () => {
    const mockContent = 'Some content';
    (llmRouter.chatCompletion as any) = vi.fn().mockResolvedValue({
      response: {
        choices: [{ message: { content: 'not a json' } }],
      },
    } as any);

    const result = await analyst.analyze(mockContent, 'test-model');

    expect(result.facts).toHaveLength(1);
    expect(result.facts[0]!.content).toBe(mockContent);
    expect(result.facts[0]!.importance).toBe(3);
    expect(result.facts[0]!.scope).toBe('personal');
  });

  it('should extract JSON from markdown blocks', async () => {
    const mockContent = 'Some content';
    const jsonResult = {
      facts: [{ title: 'Test', content: 'Test content', importance: 5, tags: [], scope: 'global' }],
    };
    (llmRouter.chatCompletion as any) = vi.fn().mockResolvedValue({
      response: {
        choices: [{ message: { content: '```json\n' + JSON.stringify(jsonResult) + '\n```' } }],
      },
    } as any);

    const result = await analyst.analyze(mockContent, 'test-model');

    expect(result.facts).toHaveLength(1);
    expect(result.facts[0]!.title).toBe('Test');
    expect(result.facts[0]!.importance).toBe(5);
    expect(result.facts[0]!.scope).toBe('global');
  });
});
