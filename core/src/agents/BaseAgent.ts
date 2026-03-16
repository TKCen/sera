import type { AgentResponse, ChatMessage } from './types.js';
import type { LLMProvider } from '../lib/llm/types.js';
import type { AgentManifest } from './manifest/types.js';
import type { IntercomService } from '../intercom/IntercomService.js';
import type { ThoughtStepType } from '../intercom/types.js';
import { IdentityService } from './identity/IdentityService.js';

export abstract class BaseAgent {
  public readonly name: string;
  public readonly role: string;

  protected history: ChatMessage[] = [];
  protected systemPrompt: string;
  protected llmProvider: LLMProvider;
  protected manifest: AgentManifest;
  protected intercom: IntercomService | undefined;

  /** Queue of incoming intercom messages for the reasoning loop. */
  protected messageQueue: Array<{ from: string; payload: Record<string, unknown> }> = [];

  constructor(
    manifest: AgentManifest,
    llmProvider: LLMProvider,
    intercom?: IntercomService,
  ) {
    this.manifest = manifest;
    this.name = manifest.metadata.displayName;
    this.role = manifest.metadata.name;
    this.llmProvider = llmProvider;
    this.intercom = intercom;
    this.systemPrompt = IdentityService.generateSystemPrompt(manifest);
  }

  public updateLlmProvider(llmProvider: LLMProvider) {
    this.llmProvider = llmProvider;
  }

  /** Attach an IntercomService after construction. */
  public setIntercom(intercom: IntercomService): void {
    this.intercom = intercom;
  }

  abstract process(input: string): Promise<AgentResponse>;

  // ── Thought Streaming ──────────────────────────────────────────────────────

  /**
   * Publish a reasoning step to the agent's thoughts channel.
   * Non-blocking — failures are logged but do not interrupt processing.
   */
  protected async publishThought(
    stepType: ThoughtStepType,
    content: string,
  ): Promise<void> {
    if (!this.intercom) return;
    try {
      await this.intercom.publishThought(
        this.role,
        this.name,
        stepType,
        content,
      );
    } catch (err) {
      console.error(`[${this.name}] Failed to publish thought:`, err);
    }
  }

  // ── Agent-to-Agent Messaging ────────────────────────────────────────────────

  /**
   * Send a direct message to a peer agent.
   * Validates permissions against the agent's intercom.canMessage list.
   */
  public async sendMessage(
    toAgent: string,
    payload: Record<string, unknown>,
  ): Promise<void> {
    if (!this.intercom) {
      console.warn(`[${this.name}] Intercom not configured, cannot send message`);
      return;
    }
    await this.intercom.sendDirectMessage(this.manifest, toAgent, payload);
  }

  /**
   * Enqueue a received message for processing in the next reasoning loop.
   */
  public onMessage(from: string, payload: Record<string, unknown>): void {
    this.messageQueue.push({ from, payload });
  }

  /** Drain and return all pending messages. */
  protected drainMessages(): Array<{ from: string; payload: Record<string, unknown> }> {
    const msgs = [...this.messageQueue];
    this.messageQueue = [];
    return msgs;
  }

  // ── Reasoning Steps (with thought streaming) ───────────────────────────────

  protected async observe(context: string): Promise<void> {
    console.log(`[${this.name}] Observing: ${context.substring(0, 50)}...`);
    this.publishThought('observe', context).catch(err => console.error(err));
  }

  protected async plan(goal: string): Promise<string> {
    console.log(`[${this.name}] Planning for goal: ${goal}`);
    this.publishThought('plan', `Planning for: ${goal}`).catch(err => console.error(err));
    return `Plan for ${goal}`;
  }

  protected async act(action: any): Promise<any> {
    console.log(`[${this.name}] Acting: ${JSON.stringify(action)}`);
    this.publishThought('act', `Executing: ${JSON.stringify(action)}`).catch(err => console.error(err));
    return { status: 'success' };
  }

  protected async reflect(outcome: any): Promise<void> {
    console.log(`[${this.name}] Reflecting on outcome: ${JSON.stringify(outcome)}`);
    this.publishThought('reflect', `Outcome: ${JSON.stringify(outcome)}`).catch(err => console.error(err));
  }
}
