import type { AgentResponse, ChatMessage } from './types.js';
import type { LLMProvider, ToolCall } from '../lib/llm/types.js';
import type { AgentManifest } from './manifest/types.js';
import type { IntercomService } from '../intercom/IntercomService.js';
import type { ThoughtStepType } from '../intercom/types.js';
import type { ToolExecutor } from '../tools/ToolExecutor.js';
import { ChannelNamespace } from '../intercom/ChannelNamespace.js';
import { IdentityService } from './identity/IdentityService.js';
import { LoopGuard } from './stability/LoopGuard.js';
import { SessionRepair } from './stability/SessionRepair.js';
import { Logger } from '../lib/logger.js';

/** Maximum tool-call loop iterations before forcing a text response. */
const MAX_TOOL_ITERATIONS = 10;

export abstract class BaseAgent {
  public readonly name: string;
  public readonly role: string;

  protected history: ChatMessage[] = [];
  protected systemPrompt: string;
  protected llmProvider: LLMProvider;
  protected manifest: AgentManifest;
  protected intercom: IntercomService | undefined;
  protected toolExecutor: ToolExecutor | undefined;
  protected logger: Logger;
  protected loopGuard: LoopGuard;

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
    this.loopGuard = new LoopGuard();
  }

  public updateLlmProvider(llmProvider: LLMProvider) {
    this.llmProvider = llmProvider;
  }

  /** Attach an IntercomService after construction. */
  public setIntercom(intercom: IntercomService): void {
    this.intercom = intercom;
  }

  /** Attach a ToolExecutor after construction. */
  public setToolExecutor(toolExecutor: ToolExecutor): void {
    this.toolExecutor = toolExecutor;
  }

  abstract process(input: string, history?: ChatMessage[]): Promise<AgentResponse>;

  // ── Streaming Process with Tool Loop ────────────────────────────────────────

  /**
   * Stream the LLM response token-by-token to a Centrifugo channel.
   * Supports agentic tool-call loop: if the LLM returns tool_calls instead of
   * content, execute them and feed results back until the LLM produces text.
   */
  async processStream(
    input: string,
    history: ChatMessage[],
    messageId: string,
  ): Promise<AgentResponse> {
    // Reset loop guard for each new request
    this.loopGuard.reset();

    await this.observe(input);
    await this.plan(input);

    // Repair message history before building the conversation
    const { messages: cleanHistory, report } = SessionRepair.repair(history);
    if (report.repaired) {
      await this.publishThought('reflect',
        `Session repaired: ${report.orphanedToolMessages} orphaned tool msgs, ` +
        `${report.emptyMessages} empty msgs, ${report.mergedMessages} merged msgs`,
      );
    }

    const messages: ChatMessage[] = [
      { role: 'system', content: IdentityService.generateStreamingSystemPrompt(this.manifest) },
      ...cleanHistory,
      { role: 'user', content: input },
    ];

    const streamChannel = ChannelNamespace.stream(messageId);

    // Get tool definitions if ToolExecutor is available
    const tools = this.toolExecutor
      ? this.toolExecutor.getToolDefinitions(this.manifest)
      : [];
    const hasTools = tools.length > 0;

    try {
      let iterations = 0;

      // ── Agentic Loop ──────────────────────────────────────────────────
      while (iterations < MAX_TOOL_ITERATIONS) {
        iterations++;

        if (hasTools) {
          // Non-streaming call that supports tools
          const response = await this.llmProvider.chat(messages, tools);

          if (response.toolCalls && response.toolCalls.length > 0) {
            // ── Tool Call Phase ────────────────────────────────────────
            // Check loop guard for each tool call
            const blockedCalls: string[] = [];
            for (const tc of response.toolCalls) {
              let parsedArgs: unknown;
              try { parsedArgs = JSON.parse(tc.function.arguments || '{}'); } catch { parsedArgs = tc.function.arguments; }
              const guard = this.loopGuard.recordCall(tc.function.name, parsedArgs);

              if (guard.status === 'block') {
                blockedCalls.push(tc.function.name);
                await this.publishThought(
                  'reflect',
                  `⛔ Blocked duplicate call: **${tc.function.name}** (${guard.count} identical calls)`,
                );
              } else if (guard.status === 'warn') {
                await this.publishThought(
                  'reflect',
                  `⚠️ Repeated call: **${tc.function.name}** (${guard.count} times) — try a different approach`,
                );
              }

              await this.publishThought(
                'tool-call',
                `Calling tool: **${tc.function.name}**(${tc.function.arguments})`,
              );
            }

            // If ALL calls are blocked, force a text response without tools
            if (blockedCalls.length === response.toolCalls.length) {
              await this.publishThought('reflect', 'All tool calls blocked by loop guard. Generating final response.');
              messages.push({
                role: 'assistant',
                content: response.content || '',
              } as ChatMessage);
              messages.push({
                role: 'user',
                content: 'Your previous tool calls were blocked because you repeated the same calls too many times. Please provide your best answer using the information you already have, without calling more tools.',
              });
              const fallback = await this.llmProvider.chat(messages);
              const reply = fallback.content || 'I was unable to complete this task — my tool calls were repeating.';
              await this.streamTextToChannel(reply, streamChannel, messageId);
              this.history = [
                ...cleanHistory,
                { role: 'user', content: input },
                { role: 'assistant', content: reply } as ChatMessage,
              ];
              return { thought: 'Loop guard blocked all tool calls', finalAnswer: reply };
            }

            // Filter out blocked calls
            const activeCalls = response.toolCalls.filter(
              tc => !blockedCalls.includes(tc.function.name),
            );

            // Add assistant message with tool_calls to the conversation
            const assistantMsg: ChatMessage = {
              role: 'assistant',
              content: response.content || '',
              tool_calls: activeCalls,
            };
            messages.push(assistantMsg);

            // Execute non-blocked tool calls
            const toolResults = await this.toolExecutor!.executeToolCalls(activeCalls);

            // Publish results and add to conversation
            for (const result of toolResults) {
              const preview = result.content.length > 200
                ? result.content.substring(0, 200) + '...'
                : result.content;
              await this.publishThought('tool-result', preview);
              messages.push(result);
            }

            // Continue the loop — LLM will see the tool results
            continue;
          }

          // ── Text Response (no tool calls) ───────────────────────────
          // Stream the final text response to the channel
          const reply = response.content || 'No response generated.';
          await this.streamTextToChannel(reply, streamChannel, messageId);

          // Update history
          this.history = [
            ...cleanHistory,
            { role: 'user', content: input },
            { role: 'assistant', content: reply } as ChatMessage,
          ];

          const result: AgentResponse = {
            thought: `Completed task: ${input}`,
            finalAnswer: reply,
          };
          await this.reflect(result);
          return result;
        } else {
          // ── No tools: simple streaming (original behavior) ──────────
          return await this.streamSimple(input, history, messages, streamChannel, messageId);
        }
      }

      // ── Max iterations reached — force a final response without tools ──
      this.logger.warn(`Tool loop hit max iterations (${MAX_TOOL_ITERATIONS}), forcing final response`);
      await this.publishThought('reflect', `Reached maximum tool iterations (${MAX_TOOL_ITERATIONS}). Generating final response.`);

      const finalResponse = await this.llmProvider.chat(messages);
      const reply = finalResponse.content || 'I was unable to complete this task within the allowed number of steps.';
      await this.streamTextToChannel(reply, streamChannel, messageId);

      this.history = [
        ...cleanHistory,
        { role: 'user', content: input },
        { role: 'assistant', content: reply } as ChatMessage,
      ];

      const result: AgentResponse = {
        thought: `Hit max tool iterations: ${input}`,
        finalAnswer: reply,
      };
      await this.reflect(result);
      return result;

    } catch (error: any) {
      this.logger.error('Stream processing error:', error);
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
  }

  // ── Private Helpers ─────────────────────────────────────────────────────────

  /**
   * Stream a text string token-by-token to a Centrifugo channel.
   * Used for the final response after tool calls complete.
   */
  private async streamTextToChannel(
    text: string,
    channel: string,
    messageId: string,
  ): Promise<void> {
    if (!this.intercom) return;

    // Send the text in chunks to simulate streaming
    const chunkSize = 20;
    for (let i = 0; i < text.length; i += chunkSize) {
      const chunk = text.substring(i, i + chunkSize);
      await this.intercom.publishStreamToken(channel, chunk, false, messageId);
    }
    await this.intercom.publishStreamToken(channel, '', true, messageId);
  }

  /**
   * Simple streaming path for agents without tools.
   * Preserves the original behavior of streaming token-by-token from the LLM.
   */
  private async streamSimple(
    input: string,
    originalHistory: ChatMessage[],
    messages: ChatMessage[],
    streamChannel: string,
    messageId: string,
  ): Promise<AgentResponse> {
    let accumulated = '';

    for await (const chunk of this.llmProvider.chatStream(messages)) {
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

    const reply = accumulated || 'No response generated.';

    this.history = [
      ...originalHistory,
      { role: 'user', content: input },
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
