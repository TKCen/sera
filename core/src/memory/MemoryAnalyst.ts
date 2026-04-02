import { LlmRouter } from '../llm/LlmRouter.js';
import { Logger } from '../lib/logger.js';
import { MemoryScope, Importance } from './blocks/scoped-types.js';

const logger = new Logger('MemoryAnalyst');

export interface AtomicFact {
  title: string;
  content: string;
  importance: Importance;
  tags: string[];
  scope: MemoryScope;
}

export interface AnalysisResult {
  facts: AtomicFact[];
}

const SYSTEM_PROMPT = `You are a memory analysis assistant. Your task is to analyze a memory entry and:
1. Extract atomic facts from the content. If the content contains multiple distinct pieces of information, split them into separate atomic facts.
2. For each atomic fact, infer:
   - A concise, descriptive title.
   - The core content (Markdown).
   - Importance on a scale of 1-5 (1: trivial, 5: critical).
   - Relevant category tags.
   - The optimal scope: 'personal' (specific to this agent), 'circle' (relevant to the agent's team/circle), or 'global' (generally applicable knowledge).

Return the results as a JSON object with the following structure:
{
  "facts": [
    {
      "title": "string",
      "content": "string",
      "importance": number,
      "tags": ["string"],
      "scope": "personal" | "circle" | "global"
    }
  ]
}`;

export class MemoryAnalyst {
  constructor(private readonly llmRouter: LlmRouter) {}

  /**
   * Analyze memory content using an LLM to extract importance, scope, tags, and atomic facts.
   */
  async analyze(content: string, modelName: string): Promise<AnalysisResult> {
    try {
      const { response } = await this.llmRouter.chatCompletion(
        {
          model: modelName,
          messages: [
            { role: 'system', content: SYSTEM_PROMPT },
            { role: 'user', content: `Analyze the following memory content:\n\n${content}` },
          ],
          temperature: 0.1,
        },
        '_memory_analyst'
      );

      const jsonContent = response.choices[0]?.message?.content;
      if (!jsonContent) {
        throw new Error('LLM returned empty response');
      }

      // Try to extract JSON from the response (in case of markdown blocks)
      const jsonMatch = jsonContent.match(/\{[\s\S]*\}/);
      const cleanedJson = jsonMatch ? jsonMatch[0] : jsonContent;

      const result = JSON.parse(cleanedJson) as AnalysisResult;

      // Validate and sanitize result
      if (!result.facts || !Array.isArray(result.facts)) {
        throw new Error('Invalid analysis result structure');
      }

      result.facts = result.facts.map(fact => ({
        title: fact.title || 'Untitled Fact',
        content: fact.content || '',
        importance: Math.max(1, Math.min(5, Math.round(fact.importance || 3))) as Importance,
        tags: Array.isArray(fact.tags) ? fact.tags : [],
        scope: (['personal', 'circle', 'global'].includes(fact.scope) ? fact.scope : 'personal') as MemoryScope,
      }));

      return result;
    } catch (err) {
      logger.error('Failed to analyze memory content:', err);
      // Fallback: return a single fact with default metadata
      return {
        facts: [
          {
            title: content.slice(0, 80).replace(/\n/g, ' '),
            content,
            importance: 3,
            tags: [],
            scope: 'personal',
          },
        ],
      };
    }
  }
}
