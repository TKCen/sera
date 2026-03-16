import { BaseAgent } from './BaseAgent.js';
import type { AgentResponse, AgentRole } from './types.js';

export class PrimaryAgent extends BaseAgent {
  constructor() {
    super('Sera-Primary', 'primary', 'You are the primary coordinator agent.');
  }

  async process(input: string): Promise<AgentResponse> {
    await this.observe(input);
    const plan = await this.plan(input);

    // Mock logic: if input contains "research", delegate
    if (input.toLowerCase().includes('research')) {
      return {
        thought: 'I need to research this topic, delegating to research agent.',
        delegation: {
          agentRole: 'researcher',
          task: `Research about: ${input}`
        }
      };
    }

    return {
      thought: 'I can handle this directly.',
      finalAnswer: `Processed: ${input}`
    };
  }
}
