import { BaseAgent } from './BaseAgent.js';
import type { AgentResponse, ChatMessage } from './types.js';
import type { LLMProvider } from '../lib/llm/types.js';
import type { AgentManifest } from './manifest/types.js';

export class WorkerAgent extends BaseAgent {
  constructor(manifest: AgentManifest, llmProvider: LLMProvider) {
    super(manifest, llmProvider);
  }

  async process(input: string, history: ChatMessage[] = []): Promise<AgentResponse> {
    await this.observe(input);
    await this.plan(input);

    const fullHistory = [...history, { role: 'user', content: input } as ChatMessage];

    const response = await this.llmProvider.chat([
      { role: 'system', content: this.systemPrompt },
      ...fullHistory
    ]);

    this.history = [...fullHistory, { role: 'assistant', content: response.content } as ChatMessage];

    try {
      const jsonMatch = response.content.match(/\{[\s\S]*\}/);
      if (jsonMatch) {
        const parsed = JSON.parse(jsonMatch[0]);
        await this.reflect({ thought: parsed.thought, finalAnswer: parsed.finalAnswer });
        return parsed;
      }
      const result: AgentResponse = {
        thought: `Completed task: ${input}`,
        finalAnswer: response.content,
      };
      await this.reflect(result);
      return result;
    } catch (error) {
      this.logger.error('Failed to parse worker response:', error);
      return {
        thought: 'Error parsing response.',
        finalAnswer: response.content
      };
    }
  }
}
