import { BaseAgent } from './BaseAgent.js';
import type { AgentResponse } from './types.js';
import type { LLMProvider } from '../lib/llm/types.js';
import type { AgentManifest } from './manifest/types.js';

export class PrimaryAgent extends BaseAgent {
  constructor(manifest: AgentManifest, llmProvider: LLMProvider) {
    super(manifest, llmProvider);
  }

  async process(input: string): Promise<AgentResponse> {
    await this.observe(input);

    this.history.push({ role: 'user', content: input });

    const response = await this.llmProvider.chat([
      { role: 'system', content: this.systemPrompt },
      ...this.history
    ]);

    this.history.push({ role: 'assistant', content: response.content });

    try {
      // Extract JSON from the response
      const jsonMatch = response.content.match(/\{[\s\S]*\}/);
      if (jsonMatch) {
        return JSON.parse(jsonMatch[0]);
      }
      return {
        thought: 'Received non-JSON response from LLM, treating as final answer.',
        finalAnswer: response.content
      };
    } catch (error) {
      console.error('Failed to parse agent response:', error);
      return {
        thought: 'Error parsing response.',
        finalAnswer: response.content
      };
    }
  }
}
