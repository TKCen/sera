import type { AgentResponse, ChatMessage } from './types.js';
import type { LLMProvider } from '../lib/llm/types.js';
import type { AgentManifest } from './manifest/types.js';
import type { IntercomService } from '../intercom/IntercomService.js';
import type { ThoughtStepType } from '../intercom/types.js';
import { ChannelNamespace } from '../intercom/ChannelNamespace.js';
import { IdentityService } from './identity/IdentityService.js';
import { Logger } from '../lib/logger.js';

export abstract class BaseAgent {
  public readonly name: string;
  public readonly role: string;

  protected history: ChatMessage[] = [];
  protected systemPrompt: string;
  protected llmProvider: LLMProvider;
  protected manifest: AgentManifest;
  protected intercom: IntercomService | undefined;
  protected logger: Logger;

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
    this.logger = new Logger(this.name);
  }

  public updateLlmProvider(llmProvider: LLMProvider) {
    this.llmProvider = llmProvider;
  }

  /** Attach an IntercomService after construction. */
  public setIntercom(intercom: IntercomService): void {
    this.intercom = intercom;
  }

  abstract process(input: string, history?: ChatMessage[]): Promise<AgentResponse>;

  // ── Streaming Process ──────────────────────────────────────────────────────

  /**
   * Stream the LLM response token-by-token to a Centrifugo channel.
   * Returns the full accumulated response for history storage.
   */
  async processStream(
    input: string,
    history: ChatMessage[],
    messageId: string,
  ): Promise<AgentResponse> {
    await this.observe(input);
    await this.plan(input);

    const fullHistory: ChatMessage[] = [
      ...history,
      { role: 'user', content: input },
    ];

    const streamChannel = ChannelNamespace.stream(messageId);
    const streamingPrompt = IdentityService.generateStreamingSystemPrompt(this.manifest);
    let accumulated = '';

    try {
      for await (const chunk of this.llmProvider.chatStream([
        { role: 'system', content: streamingPrompt },
        ...fullHistory,
      ])) {
        if (chunk.token) {
          accumulated += chunk.token;
        }

        if (this.intercom) {
          await this.intercom.publishStreamToken(
            streamChannel,
            chunk.token,
            chunk.done,
            messageId,
          );
        }
      }
    } catch (error: any) {
      this.logger.error('Stream processing error:', error);
      // Send error as final token
      if (this.intercom) {
        await this.intercom.publishStreamToken(
          streamChannel,
          `⚠️ Error: ${error.message}`,
          true,
          messageId,
        );
      }
      return {
        thought: 'Error during streaming.',
        finalAnswer: `Error: ${error.message}`,
      };
    }

    const reply = accumulated || 'No response generated.';

    this.history = [
      ...fullHistory,
      { role: 'assistant', content: reply } as ChatMessage,
    ];

    const result: AgentResponse = {
      thought: `Completed task: ${input}`,
      finalAnswer: reply,
    };
    await this.reflect(result);
    return result;
  }

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
      this.logger.error(`Failed to publish thought:`, err);
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
      this.logger.warn(`Intercom not configured, cannot send message`);
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
    this.logger.debug(`Observing: ${context.substring(0, 50)}...`);
    await this.publishThought('observe', context);
  }

  protected async plan(goal: string): Promise<string> {
    this.logger.debug(`Planning for goal: ${goal}`);
    await this.publishThought('plan', `Planning for: ${goal}`);
    return `Plan for ${goal}`;
  }

  protected async act(action: any): Promise<any> {
    this.logger.debug(`Acting: ${JSON.stringify(action)}`);
    await this.publishThought('act', `Executing: ${JSON.stringify(action)}`);
    return { status: 'success' };
  }

  protected async reflect(outcome: any): Promise<void> {
    this.logger.debug(`Reflecting on outcome: ${JSON.stringify(outcome)}`);
    await this.publishThought('reflect', `Outcome: ${JSON.stringify(outcome)}`);
  }
}
