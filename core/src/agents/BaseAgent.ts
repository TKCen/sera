import type { AgentRole, AgentResponse, ChatMessage, AgentTask } from './types.js';

export abstract class BaseAgent {
  constructor(
    public name: string,
    public role: AgentRole,
    protected systemPrompt: string
  ) {}

  protected history: ChatMessage[] = [];

  abstract process(input: string): Promise<AgentResponse>;

  protected async observe(context: string): Promise<void> {
    console.log(`[${this.name}] Observing: ${context.substring(0, 50)}...`);
  }

  protected async plan(goal: string): Promise<string> {
    console.log(`[${this.name}] Planning for goal: ${goal}`);
    return `Plan for ${goal}`;
  }

  protected async act(action: any): Promise<any> {
    console.log(`[${this.name}] Acting: ${JSON.stringify(action)}`);
    return { status: 'success' };
  }

  protected async reflect(outcome: any): Promise<void> {
    console.log(`[${this.name}] Reflecting on outcome: ${JSON.stringify(outcome)}`);
  }
}
