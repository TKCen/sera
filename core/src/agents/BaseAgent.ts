import type { AgentResponse, CapturedThought, ChatMessage } from './types.js';
import type { LLMProvider } from '../lib/llm/types.js';
import type { AgentManifest } from './manifest/types.js';
import type { IntercomService } from '../intercom/IntercomService.js';
import type { ThoughtStepType } from '../intercom/types.js';
import type { ToolExecutor } from '../tools/ToolExecutor.js';
import type { MemoryManager } from '../memory/manager.js';
import type { ContextCompactionService } from '../llm/ContextCompactionService.js';
import { ChannelNamespace } from '../intercom/ChannelNamespace.js';
import { IdentityService } from './identity/IdentityService.js';
import { LoopGuard } from './stability/LoopGuard.js';
import { SessionRepair } from './stability/SessionRepair.js';
import type { MeteringEngine } from '../metering/MeteringEngine.js';
import type { AgentScheduler } from '../metering/AgentScheduler.js';
import { Logger } from '../lib/logger.js';
import { AuditService } from '../audit/AuditService.js';

/** Maximum tool-call loop iterations before forcing a text response. */
const MAX_TOOL_ITERATIONS = 10;

export abstract class BaseAgent {
  public readonly name: string;
  public readonly role: string;
  public readonly agentInstanceId: string | undefined;
  public containerId: string | undefined;
  public readonly startTime: Date = new Date();
  public status: 'running' | 'stopped' | 'error' | 'unresponsive' | 'throttled' = 'running';

  protected history: ChatMessage[] = [];
  protected systemPrompt: string;
  protected llmProvider: LLMProvider;
  protected manifest: AgentManifest;
  protected intercom: IntercomService | undefined;
  protected toolExecutor: ToolExecutor | undefined;
  protected meteringEngine: MeteringEngine | undefined;
  protected agentScheduler: AgentScheduler | undefined;
  protected memoryManager: MemoryManager | undefined;
  protected logger: Logger;
  protected loopGuard: LoopGuard;
  protected identityService: IdentityService | undefined;
  protected contextCompactionService: ContextCompactionService | undefined;

  /** Queue of incoming intercom messages for the reasoning loop. */
  protected messageQueue: Array<{ from: string; payload: Record<string, unknown> }> = [];

  /** Thoughts collected during the current processStream call (cleared on each invocation). */
  private _capturedThoughts: CapturedThought[] = [];
  /** Optional per-request thought listener set during processStream. */
  private _onThoughtCallback: ((t: CapturedThought) => void) | undefined;

  constructor(
    manifest: AgentManifest,
    llmProvider: LLMProvider,
    intercom?: IntercomService,
    agentInstanceId?: string,
    memoryManager?: MemoryManager
  ) {
    this.manifest = manifest;
    this.name = manifest.metadata.displayName;
    this.role = manifest.metadata.name;
    this.llmProvider = llmProvider;
    this.intercom = intercom;
    this.agentInstanceId = agentInstanceId;
    this.memoryManager = memoryManager;
    this.systemPrompt = IdentityService.generateSystemPrompt(manifest);
    this.logger = new Logger(this.name);
    this.loopGuard = new LoopGuard();
  }

  public getManifest(): AgentManifest {
    return this.manifest;
  }

  public setContainerId(containerId: string | undefined): void {
    this.containerId = containerId;
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

  /** Attach metering components after construction. */
  public setMetering(engine: MeteringEngine, scheduler: AgentScheduler): void {
    this.meteringEngine = engine;
    this.agentScheduler = scheduler;
  }

  /** Attach a MemoryManager after construction. */
  public setMemoryManager(memoryManager: MemoryManager): void {
    this.memoryManager = memoryManager;
  }

  /** Attach a ContextCompactionService for automatic context window management. */
  public setContextCompactionService(service: ContextCompactionService): void {
    this.contextCompactionService = service;
  }

  /** Optional resolver for circle project context. Set by Orchestrator on startup. */
  protected circleContextResolver: (() => string | undefined) | undefined;

  /** Attach a circle context resolver after construction. */
  public setCircleContextResolver(resolver: () => string | undefined): void {
    this.circleContextResolver = resolver;
  }

  /** Attach an IdentityService after construction. */
  public setIdentityService(identityService: IdentityService): void {
    this.identityService = identityService;
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
    onThought?: (thought: CapturedThought) => void
  ): Promise<AgentResponse> {
    // Clear captured thoughts for this request
    this._capturedThoughts = [];
    this._onThoughtCallback = onThought;
    // Reset loop guard for each new request
    this.loopGuard.reset();

    await this.observe(input);
    await this.plan(input);

    // Repair message history before building the conversation
    const { messages: cleanHistory, report } = SessionRepair.repair(history);
    if (report.repaired) {
      await this.publishThought(
        'reflect',
        `Session repaired: ${report.orphanedToolMessages} orphaned tool msgs, ` +
          `${report.emptyMessages} empty msgs, ${report.mergedMessages} merged msgs`
      );
    }

    let dynamicContext = '';
    let memoryDegraded = false;
    if (this.memoryManager) {
      try {
        dynamicContext = await this.memoryManager.assembleContext(input);
      } catch {
        memoryDegraded = true;
        this.logger.warn('Memory context unavailable — embedding service may be down');
      }
    }

    // Resolve circle project context (if agent belongs to a circle)
    const circleContext = this.circleContextResolver?.();

    // Add degradation notice if memory is unavailable
    const degradationNotice = memoryDegraded
      ? '\n\n**Note:** Knowledge search is currently unavailable (embedding service down). ' +
        'Do not attempt to use knowledge-query or knowledge-store tools.'
      : '';

    const messages: ChatMessage[] = [
      {
        role: 'system',
        content:
          IdentityService.generateStreamingSystemPrompt(
            this.manifest,
            circleContext,
            dynamicContext
          ) + degradationNotice,
      },
      ...cleanHistory,
      { role: 'user', content: input },
    ];

    const streamChannel = ChannelNamespace.tokens(this.agentInstanceId || this.role);

    // ── Quota Check ───────────────────────────────────────────────────
    if (this.agentScheduler && this.manifest.resources?.maxLlmTokensPerHour) {
      const allowed = await this.agentScheduler.isWithinQuota(
        this.agentInstanceId || this.role,
        this.manifest.resources.maxLlmTokensPerHour
      );
      if (!allowed) {
        const errorMsg = '⚠️ Hourly token quota exceeded. Request denied.';
        await this.publishThought('reflect', errorMsg);
        await this.streamTextToChannel(errorMsg, streamChannel, messageId);
        return { thought: 'Quota exceeded', finalAnswer: errorMsg };
      }
    }

    // Get tool definitions if ToolExecutor is available
    const tools = this.toolExecutor ? this.toolExecutor.getToolDefinitions(this.manifest) : [];
    const hasTools = tools.length > 0;

    try {
      let iterations = 0;

      // ── Agentic Loop ──────────────────────────────────────────────────
      while (iterations < MAX_TOOL_ITERATIONS) {
        iterations++;

        if (hasTools) {
          // Non-streaming call that supports tools
          const response = await this.llmProvider.chat(messages, tools);

          // Record usage for the tool-call prompt
          if (response.usage && this.meteringEngine) {
            await this.meteringEngine.record({
              agentId: this.agentInstanceId || this.role,
              model: this.manifest.model.name,
              ...response.usage,
            });
          }

          if (response.toolCalls && response.toolCalls.length > 0) {
            // ── Tool Call Phase ────────────────────────────────────────
            // Check loop guard for each tool call
            const blockedCalls: string[] = [];
            for (const tc of response.toolCalls) {
              let parsedArgs: unknown;
              try {
                parsedArgs = JSON.parse(tc.function.arguments || '{}');
              } catch {
                parsedArgs = tc.function.arguments;
              }
              const guard = this.loopGuard.recordCall(tc.function.name, parsedArgs);

              if (guard.status === 'block') {
                blockedCalls.push(tc.function.name);
                await this.publishThought(
                  'reflect',
                  `⛔ Blocked duplicate call: **${tc.function.name}** (${guard.count} identical calls)`
                );
              } else if (guard.status === 'warn') {
                await this.publishThought(
                  'reflect',
                  `⚠️ Repeated call: **${tc.function.name}** (${guard.count} times) — try a different approach`
                );
              }

              await this.publishThought(
                'tool-call',
                `Tool: ${tc.function.name}\nParameters: ${tc.function.arguments}`
              );
            }

            // If ALL calls are blocked, force a text response without tools
            if (blockedCalls.length === response.toolCalls.length) {
              await this.publishThought(
                'reflect',
                'All tool calls blocked by loop guard. Generating final response.'
              );
              messages.push({
                role: 'assistant',
                content: response.content || '',
              } as ChatMessage);
              messages.push({
                role: 'user',
                content:
                  'Your previous tool calls were blocked because you repeated the same calls too many times. Please provide your best answer using the information you already have, without calling more tools.',
              });
              const fallback = await this.llmProvider.chat(messages);

              if (fallback.usage && this.meteringEngine) {
                await this.meteringEngine.record({
                  agentId: this.agentInstanceId || this.role,
                  model: this.manifest.model.name,
                  ...fallback.usage,
                });
              }

              const reply =
                fallback.content ||
                'I was unable to complete this task — my tool calls were repeating.';
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
              (tc) => !blockedCalls.includes(tc.function.name)
            );

            // Add assistant message with tool_calls to the conversation
            const assistantMsg: ChatMessage = {
              role: 'assistant',
              content: response.content || '',
              tool_calls: activeCalls,
            };
            messages.push(assistantMsg);

            // Execute non-blocked tool calls
            const toolResults = await this.toolExecutor!.executeToolCalls(
              activeCalls,
              this.manifest,
              this.agentInstanceId,
              this.containerId,
              messageId
            );

            // Record audit entries for tool calls in parallel
            await Promise.all(
              activeCalls.map(async (tc, i) => {
                const tr = toolResults[i]!;
                let trContent = '';
                if (typeof tr.content === 'string') {
                  trContent = tr.content;
                } else if (Array.isArray(tr.content)) {
                  trContent = '[multi-part content]';
                }

                try {
                  await AuditService.getInstance().record({
                    actorType: 'agent',
                    actorId: this.agentInstanceId || this.name,
                    actingContext: null,
                    eventType: 'tool.called',
                    payload: {
                      tool: tc.function.name,
                      args: tc.function.arguments,
                      result:
                        trContent.length > 500 ? trContent.substring(0, 500) + '...' : trContent,
                    },
                  });
                } catch (auditErr) {
                  this.logger.error('Failed to record audit entry:', auditErr);
                }
              })
            );

            // Publish results and add to conversation
            for (const result of toolResults) {
              let preview = '';
              if (typeof result.content === 'string') {
                preview =
                  result.content.length > 2000
                    ? result.content.substring(0, 2000) + '...'
                    : result.content;
              } else if (Array.isArray(result.content)) {
                preview = '[multi-part content]';
              }
              await this.publishThought('tool-result', `Result: ${preview}`);
              messages.push(result);
            }

            // Continue the loop — LLM will see the tool results
            continue;
          }

          // ── Text Response (no tool calls) ───────────────────────────
          // Stream the final text response to the channel
          // The streamSimple handles history update and returns AgentResponse
          const result = await this.streamSimple(
            input,
            cleanHistory,
            messages,
            streamChannel,
            messageId
          );
          return result;
        } else {
          // ── No tools: simple streaming (original behavior) ──────────
          return await this.streamSimple(input, history, messages, streamChannel, messageId);
        }
      }

      // ── Max iterations reached — force a final response without tools ──
      this.logger.warn(
        `Tool loop hit max iterations (${MAX_TOOL_ITERATIONS}), forcing final response`
      );
      await this.publishThought(
        'reflect',
        `Reached maximum tool iterations (${MAX_TOOL_ITERATIONS}). Generating final response.`
      );

      return await this.streamSimple(input, cleanHistory, messages, streamChannel, messageId);
    } catch (error: unknown) {
      const errorMessage = error instanceof Error ? error.message : String(error);
      this.logger.error('Stream processing error:', error);
      if (this.intercom) {
        await this.intercom.publishToken(
          this.agentInstanceId || this.role,
          `⚠️ Error: ${errorMessage}`,
          true,
          messageId
        );
      }
      return {
        thought: 'Error during streaming.',
        finalAnswer: `Error: ${errorMessage}`,
        thoughts: this._capturedThoughts.slice(),
      };
    } finally {
      this._onThoughtCallback = undefined;
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
    messageId: string
  ): Promise<void> {
    if (!this.intercom) return;

    // Send the text in chunks to simulate streaming
    const chunkSize = 20;
    for (let i = 0; i < text.length; i += chunkSize) {
      const chunk = text.substring(i, i + chunkSize);
      await this.intercom.publishToken(this.agentInstanceId || this.role, chunk, false, messageId);
    }
    await this.intercom.publishToken(this.agentInstanceId || this.role, '', true, messageId);
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
    messageId: string
  ): Promise<AgentResponse> {
    let accumulated = '';
    let accumulatedReasoning = '';

    // Context compaction: summarize/trim if over context window budget
    if (this.contextCompactionService) {
      try {
        const modelName = this.manifest.spec?.model?.name ?? this.manifest.model?.name ?? 'default';
        const compacted = await this.contextCompactionService.compact(
          messages as import('../llm/LlmRouter.js').ChatMessage[],
          modelName,
          (event) => {
            if ((event.stage as string) !== 'compaction.skipped') {
              void this.publishThought('context-assembly', JSON.stringify(event));
            }
          }
        );
        messages = compacted as ChatMessage[];
      } catch (err) {
        this.logger.error(`[${this.role}] Context compaction failed:`, err);
      }
    }

    try {
      for await (const chunk of this.llmProvider.chatStream(messages)) {
        if (chunk.token) {
          accumulated += chunk.token;

          // First content token: flush any accumulated reasoning as one thought block
          if (accumulatedReasoning) {
            await this.publishThought('reasoning', accumulatedReasoning);
            accumulatedReasoning = '';
          }
        }

        // Accumulate reasoning/thinking tokens (e.g. Qwen / DeepSeek reasoning_content)
        if (chunk.reasoning) {
          accumulatedReasoning += chunk.reasoning;
        }

        if (chunk.usage && this.meteringEngine) {
          await this.meteringEngine.record({
            agentId: this.agentInstanceId || this.role,
            model: this.manifest.model.name,
            ...chunk.usage,
          });
        }

        // Only publish to the frontend when there is an actual token or the stream
        // is done. Skipping empty-string tokens avoids flooding Centrifugo with
        // hundreds of no-op HTTP requests during a thinking/reasoning phase
        // (e.g. Qwen3 emits reasoning_content before any visible content).
        if (this.intercom && (chunk.token !== '' || chunk.done)) {
          await this.intercom.publishToken(
            this.agentInstanceId || this.role,
            chunk.token,
            chunk.done,
            messageId
          );
        }
      }

      // Flush any remaining reasoning (e.g. model only reasoned, produced no content)
      if (accumulatedReasoning) {
        await this.publishThought('reasoning', accumulatedReasoning);

        // Fallback: if the model only produced reasoning tokens and no content tokens
        // (happens when reasoning flag is misconfigured), use the reasoning as the reply.
        if (!accumulated) {
          accumulated = accumulatedReasoning;
          // Also publish the reasoning as token content so the web UI shows it
          if (this.intercom) {
            await this.intercom.publishToken(
              this.agentInstanceId || this.role,
              accumulated,
              false,
              messageId
            );
          }
        }
      }
    } catch (streamError) {
      const errMsg = streamError instanceof Error ? streamError.message : String(streamError);
      this.logger.error(`[${this.role}] LLM stream error:`, errMsg);
      const userMsg =
        '⚠️ LLM error: ' +
        (errMsg.includes('context') || errMsg.includes('n_keep') || errMsg.includes('n_ctx')
          ? 'Context window exceeded. Try a shorter message or clear the conversation.'
          : errMsg.length > 200
            ? errMsg.substring(0, 200) + '...'
            : errMsg);
      await this.publishThought('reflect', userMsg);
      if (this.intercom) {
        await this.intercom.publishToken(
          this.agentInstanceId || this.role,
          userMsg,
          true,
          messageId
        );
      }
      return { thought: 'LLM error', finalAnswer: userMsg };
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
      thoughts: this._capturedThoughts.slice(),
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
    taskId?: string,
    iteration?: number
  ): Promise<void> {
    const timestamp = new Date().toISOString();
    // Capture for session persistence and notify external listener
    this._capturedThoughts.push({ timestamp, stepType, content });
    this._onThoughtCallback?.({ timestamp, stepType, content });
    if (!this.intercom) return;
    try {
      await this.intercom.publishThought(
        this.agentInstanceId || this.role,
        this.name,
        stepType,
        content,
        taskId,
        iteration
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
  public async sendMessage(toAgent: string, payload: Record<string, unknown>): Promise<void> {
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
    const observation = `Goal: ${context.substring(0, 100)}${context.length > 100 ? '...' : ''}`;
    this.logger.debug(observation);
    await this.publishThought('observe', observation);
  }

  protected async plan(goal: string): Promise<string> {
    const tools = this.manifest.tools?.allowed?.join(', ') || 'none';
    const planDescription = `Strategy: Using tools [${tools}] to address: ${goal.substring(0, 50)}...`;
    this.logger.debug(planDescription);
    await this.publishThought('plan', planDescription);
    return planDescription;
  }

  protected async act(action: unknown): Promise<unknown> {
    this.logger.debug(`Acting: ${JSON.stringify(action)}`);
    await this.publishThought('act', `Executing: ${JSON.stringify(action)}`);
    return { status: 'success' };
  }

  protected async reflect(outcome: unknown): Promise<void> {
    this.logger.debug(`Reflecting on outcome: ${JSON.stringify(outcome)}`);
    await this.publishThought('reflect', `Outcome: ${JSON.stringify(outcome)}`);
  }
}
