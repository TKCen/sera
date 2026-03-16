import { BaseAgent } from './BaseAgent.js';
import type { AgentResponse, AgentRole } from './types.js';

export class WorkerAgent extends BaseAgent {
  constructor(name: string, role: AgentRole) {
    super(name, role, `You are a specialized worker agent with role: ${role}`);
  }

  async process(input: string): Promise<AgentResponse> {
    await this.observe(input);
    console.log(`[${this.name}] Working on specialized task...`);

    return {
      thought: `I have completed the task: ${input}`,
      finalAnswer: `Specialized result for: ${input}`
    };
  }
}
