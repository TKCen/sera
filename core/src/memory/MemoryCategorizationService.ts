import { Logger } from '../lib/logger.js';
import type { LlmRouter } from '../llm/LlmRouter.js';
import type { KnowledgeBlockType, Importance } from './blocks/scoped-types.js';
import { KNOWLEDGE_BLOCK_TYPES } from './blocks/scoped-types.js';
import { parseJson } from '../lib/json.js';

const logger = new Logger('MemoryCategorizationService');

export interface AtomicFact {
  title: string;
  content: string;
  type: KnowledgeBlockType;
  tags: string[];
  importance: Importance;
}

const CATEGORIZATION_PROMPT = `Analyze the following content and extract one or more atomic knowledge blocks.
Split complex or multi-topic content into separate, self-contained facts.
For each block, infer the optimal category, importance (1-5), and relevant tags.

Available categories: ${KNOWLEDGE_BLOCK_TYPES.join(', ')}

Return a JSON array of objects with this structure:
[
  {
    "title": "Short descriptive title",
    "content": "The actual knowledge content (Markdown)",
    "type": "one of the available categories",
    "tags": ["tag1", "tag2"],
    "importance": 3
  }
]

Content to analyze:
{CONTENT}
`;

export class MemoryCategorizationService {
  /**
   * Use an LLM to split and categorize the provided content into atomic facts.
   */
  static async categorize(
    content: string,
    modelName: string,
    router: LlmRouter,
    agentId: string = 'system'
  ): Promise<AtomicFact[]> {
    const prompt = CATEGORIZATION_PROMPT.replace('{CONTENT}', content);

    try {
      const { response } = await router.chatCompletion(
        {
          model: modelName,
          messages: [
            {
              role: 'system',
              content: 'You are a knowledge extraction assistant. Output ONLY valid JSON.',
            },
            { role: 'user', content: prompt },
          ],
          temperature: 0.1, // Low temperature for consistent JSON output
        },
        agentId
      );

      const jsonStr = response.choices[0]?.message?.content || '[]';
      const results = parseJson(jsonStr);

      if (!Array.isArray(results)) {
        logger.warn('LLM did not return an array for categorization, falling back.');
        return [];
      }

      // Validate and sanitize results
      return results
        .map((item: any) => {
          const type = KNOWLEDGE_BLOCK_TYPES.includes(item.type) ? item.type : 'fact';
          const importance =
            typeof item.importance === 'number'
              ? (Math.max(1, Math.min(5, Math.round(item.importance))) as Importance)
              : 3;

          return {
            title: typeof item.title === 'string' ? item.title : content.slice(0, 50),
            content: typeof item.content === 'string' ? item.content : content,
            type,
            tags: Array.isArray(item.tags)
              ? item.tags.filter((t: any) => typeof t === 'string')
              : [],
            importance,
          };
        })
        .filter((fact) => fact.content.trim().length > 0);
    } catch (err) {
      logger.error('Memory categorization failed:', err);
      return [];
    }
  }
}
