import { describe, it, expect, vi, beforeEach } from 'vitest';
import { MemoryCategorizationService } from './MemoryCategorizationService.js';
import type { LlmRouter } from '../llm/LlmRouter.js';

describe('MemoryCategorizationService', () => {
  let mockRouter: any;

  beforeEach(() => {
    mockRouter = {
      chatCompletion: vi.fn(),
    };
  });

  it('should extract atomic facts from content', async () => {
    const mockResponse = {
      response: {
        choices: [
          {
            message: {
              content: JSON.stringify([
                {
                  title: 'Fact 1',
                  content: 'Content 1',
                  type: 'fact',
                  tags: ['tag1'],
                  importance: 4,
                },
                {
                  title: 'Fact 2',
                  content: 'Content 2',
                  type: 'insight',
                  tags: ['tag2'],
                  importance: 2,
                },
              ]),
            },
          },
        ],
      },
    };

    mockRouter.chatCompletion.mockResolvedValue(mockResponse);

    const facts = await MemoryCategorizationService.categorize(
      'Some long content',
      'gpt-4o',
      mockRouter as unknown as LlmRouter
    );

    expect(facts).toHaveLength(2);
    expect(facts[0]).toEqual({
      title: 'Fact 1',
      content: 'Content 1',
      type: 'fact',
      tags: ['tag1'],
      importance: 4,
    });
    expect(facts[1]).toEqual({
      title: 'Fact 2',
      content: 'Content 2',
      type: 'insight',
      tags: ['tag2'],
      importance: 2,
    });
  });

  it('should handle malformed JSON response', async () => {
    mockRouter.chatCompletion.mockResolvedValue({
      response: {
        choices: [{ message: { content: 'not a json' } }],
      },
    });

    const facts = await MemoryCategorizationService.categorize(
      'Some content',
      'gpt-4o',
      mockRouter as unknown as LlmRouter
    );

    expect(facts).toHaveLength(0);
  });

  it('should sanitize LLM output', async () => {
    const mockResponse = {
      response: {
        choices: [
          {
            message: {
              content: JSON.stringify([
                {
                  title: 123, // Invalid type
                  content: 'Valid content',
                  type: 'invalid-category', // Invalid category
                  importance: 10, // Out of range
                },
              ]),
            },
          },
        ],
      },
    };

    mockRouter.chatCompletion.mockResolvedValue(mockResponse);

    const facts = await MemoryCategorizationService.categorize(
      'Some content',
      'gpt-4o',
      mockRouter as unknown as LlmRouter
    );

    expect(facts).toHaveLength(1);
    expect(facts[0].type).toBe('fact'); // Fallback
    expect(facts[0].importance).toBe(5); // Capped
    expect(typeof facts[0].title).toBe('string');
  });
});
