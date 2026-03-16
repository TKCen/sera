import type { AgentRole, AgentResponse, ChatMessage, AgentTask } from './types.js';
import type { LLMProvider } from '../lib/llm/types.js';

export abstract class BaseAgent {
  protected history: ChatMessage[] = [];

  constructor(
    public name: string,
    public role: AgentRole,
    protected systemPrompt: string,
    protected llmProvider: LLMProvider
  ) {}

  public updateLlmProvider(llmProvider: LLMProvider) {
    this.llmProvider = llmProvider;
  }

  abstract process(input: string, onChunk?: (chunk: string) => void): Promise<AgentResponse>;

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
