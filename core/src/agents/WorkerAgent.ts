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

    const fullHistory = [...history, { role: 'user', content: input } as ChatMessage];

    const response = await this.llmProvider.chat([
      { role: 'system', content: this.systemPrompt },
      ...fullHistory
    ]);

    this.history = [...fullHistory, { role: 'assistant', content: response.content } as ChatMessage];

    try {
      const jsonMatch = response.content.match(/\{[\s\S]*\}/);
      if (jsonMatch) {
        return JSON.parse(jsonMatch[0]);
      }
      return {
        thought: `Completed task: ${input}`,
        finalAnswer: response.content
      };
    } catch (error) {
      console.error('Failed to parse worker response:', error);
      return {
        thought: 'Error parsing response.',
        finalAnswer: response.content
      };
    }
  }
}
