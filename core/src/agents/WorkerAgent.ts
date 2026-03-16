import { BaseAgent } from './BaseAgent.js';
import type { AgentResponse, AgentRole } from './types.js';
import type { LLMProvider } from '../lib/llm/types.js';

export class WorkerAgent extends BaseAgent {
  constructor(name: string, role: AgentRole, llmProvider: LLMProvider) {
    super(name, role, `You are a specialized worker agent with role: ${role}`, llmProvider);
  }

  async process(input: string, onChunk?: (chunk: string) => void): Promise<AgentResponse> {
    await this.observe(input);
    console.log(`[${this.name}] Working on specialized task...`);

    if (onChunk) {
      onChunk(`[${this.name}] Working on specialized task...`);
    }

    return {
      thought: `I have completed the task: ${input}`,
      finalAnswer: `Specialized result for: ${input}`
    };
  }
}
