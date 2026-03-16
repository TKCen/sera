import type { AgentResponse, ChatMessage } from './types.js';
import type { LLMProvider } from '../lib/llm/types.js';
import type { AgentManifest } from './manifest/types.js';
import { IdentityService } from './identity/IdentityService.js';

export abstract class BaseAgent {
  public readonly name: string;
  public readonly role: string;

  protected history: ChatMessage[] = [];
  protected systemPrompt: string;
  protected llmProvider: LLMProvider;
  protected manifest: AgentManifest;

  constructor(manifest: AgentManifest, llmProvider: LLMProvider) {
    this.manifest = manifest;
    this.name = manifest.metadata.displayName;
    this.role = manifest.metadata.name;
    this.llmProvider = llmProvider;
    this.systemPrompt = IdentityService.generateSystemPrompt(manifest);
  }

  public updateLlmProvider(llmProvider: LLMProvider) {
    this.llmProvider = llmProvider;
  }

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
