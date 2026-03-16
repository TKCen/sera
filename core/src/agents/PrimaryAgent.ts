import { BaseAgent } from './BaseAgent.js';
import type { AgentResponse, AgentRole } from './types.js';
import type { LLMProvider } from '../lib/llm/types.js';

export class PrimaryAgent extends BaseAgent {
  constructor(llmProvider: LLMProvider) {
    super(
      'Sera-Primary',
      'primary',
      `You are the primary coordinator agent of SERA (Sandboxed Extensible Reasoning Agent).
Your goal is to understand user requests and either handle them directly or delegate to specialized workers.
You MUST respond in JSON format with the following structure:
{
  "thought": "your inner monologue",
  "delegation": { "agentRole": "worker|researcher", "task": "description" } // optional
  "finalAnswer": "your response to the user" // optional
}`,
      llmProvider
    );
  }

  async process(input: string): Promise<AgentResponse> {
    await this.observe(input);

    this.history.push({ role: 'user', content: input });

    const stream = this.llmProvider.chat([
      { role: 'system', content: this.systemPrompt },
      ...this.history
    ]);

    let fullContent = '';
    for await (const chunk of stream) {
      fullContent += chunk;
    }

    this.history.push({ role: 'assistant', content: fullContent });

    try {
      // Basic extraction of JSON from the response
      const jsonMatch = fullContent.match(/\{[\s\S]*\}/);
      if (jsonMatch) {
        return JSON.parse(jsonMatch[0]);
      }
      return {
        thought: 'Received non-JSON response from LLM, assuming it is the final answer.',
        finalAnswer: fullContent
      };
    } catch (error) {
      console.error('Failed to parse agent response:', error);
      return {
        thought: 'I encountered an error parsing my own thoughts.',
        finalAnswer: fullContent
      };
    }
  }
}
