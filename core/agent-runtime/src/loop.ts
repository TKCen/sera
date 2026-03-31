/**
 * ReasoningLoop — the Observe → Plan → Act → Reflect cycle inside the container.
 *
 * Accepts a structured task, runs the LLM + tool loop up to MAX_ITERATIONS,
 * and returns a structured result with usage stats.
 */

import type { LLMClient, ChatMessage, ToolDefinition, LLMResponse } from './llmClient.js';
import { BudgetExceededError, ProviderUnavailableError } from './llmClient.js';
import type { RuntimeToolExecutor } from './tools/index.js';
import type { CentrifugoPublisher } from './centrifugo.js';
import type { RuntimeManifest } from './manifest.js';
import { generateSystemPrompt } from './manifest.js';
import { ContextManager } from './contextManager.js';
import { log } from './logger.js';

// ── Types ─────────────────────────────────────────────────────────────────────

export interface TaskInput {
  taskId: string;
  task: string;
  context?: string;
  history?: ChatMessage[];
}

export interface TaskOutput {
  taskId: string;
  result: string | null;
  error?: string;
  usage: {
    promptTokens: number;
    completionTokens: number;
    totalTokens: number;
  };
  /** Ordered list of thought events for Story 5.9. */
  thoughtStream: Array<{ step: string; content: string; iteration: number; timestamp: string }>;
  exitReason: 'success' | 'max_iterations_exceeded' | 'budget_exceeded' | 'provider_unavailable' | 'error' | 'shutdown';
}

// ── Constants ─────────────────────────────────────────────────────────────────

const MAX_ITERATIONS = 10;

// ── ReasoningLoop ─────────────────────────────────────────────────────────────

export class ReasoningLoop {
  private llm: LLMClient;
  private tools: RuntimeToolExecutor;
  private centrifugo: CentrifugoPublisher;
  private manifest: RuntimeManifest;
  private systemPrompt: string;
  private toolDefs: ToolDefinition[];
  private contextManager: ContextManager;

  /** Set to true when SIGTERM received; loop exits after current step. */
  shutdownRequested = false;

  constructor(
    llm: LLMClient,
    tools: RuntimeToolExecutor,
    centrifugo: CentrifugoPublisher,
    manifest: RuntimeManifest,
  ) {
    this.llm = llm;
    this.tools = tools;
    this.centrifugo = centrifugo;
    this.manifest = manifest;
    this.systemPrompt = generateSystemPrompt(manifest);
    this.toolDefs = tools.getToolDefinitions(manifest.tools?.allowed);
    this.contextManager = new ContextManager(manifest.model.name);
  }

  /**
   * Queue of incoming intercom messages to be injected into the loop.
   */
  private incomingMessages: Array<{ source: string; content: string; channel?: string }> = [];

  /**
   * Inject a message from another agent into the next reasoning step.
   */
  public receiveIncomingMessage(from: string, content: string, channel?: string): void {
    log('info', `Received message from ${from}${channel ? ` on ${channel}` : ''}: ${content.substring(0, 50)}...`);
    this.incomingMessages.push({ source: from, content, channel });
  }

  /**
   * Run the reasoning loop for the given task.
   */
  async run(input: TaskInput): Promise<TaskOutput> {
    const { taskId, task, context, history = [] } = input;
    const hasTools = this.toolDefs.length > 0;
    const thoughtStream: TaskOutput['thoughtStream'] = [];

    let totalPromptTokens = 0;
    let totalCompletionTokens = 0;

    // Helper to emit and record a thought
    const think = async (
      step: 'observe' | 'plan' | 'act' | 'reflect',
      content: string,
      iteration: number,
      opts?: { toolName?: string; toolArgs?: Record<string, unknown>; anomaly?: boolean },
    ) => {
      thoughtStream.push({ step, content, iteration, timestamp: new Date().toISOString() });
      await this.centrifugo.publishThought(step, content, iteration, opts);
    };

    // Build initial message array
    const messages: ChatMessage[] = [
      { role: 'system', content: this.systemPrompt },
      ...history,
      {
        role: 'user',
        content: context ? `${context}\n\n${task}` : task,
      },
    ];

    const toolNames = this.toolDefs.map((t) => t.function.name).join(', ') || 'none';
    await think('observe', `Received task: "${task.substring(0, 100)}${task.length > 100 ? '...' : ''}"`, 0);
    await think('plan', `Planning approach. Available tools: ${toolNames}`, 0);

    // Track last tool call to detect duplicate-call loops
    let lastToolCallSignature: string | null = null;

    try {
      let iteration = 0;

      while (iteration < MAX_ITERATIONS) {
        if (this.shutdownRequested) {
          await think('reflect', 'Shutdown requested — stopping reasoning loop', iteration, { anomaly: false });
          return {
            taskId,
            result: null,
            error: 'shutdown',
            usage: { promptTokens: totalPromptTokens, completionTokens: totalCompletionTokens, totalTokens: totalPromptTokens + totalCompletionTokens },
            thoughtStream,
            exitReason: 'shutdown',
          };
        }

        iteration++;

        // Inject incoming messages as 'user' observations
        while (this.incomingMessages.length > 0) {
          const msg = this.incomingMessages.shift()!;
          const chanSuffix = msg.channel ? ` (on ${msg.channel})` : '';
          const content = `[INTERCOM] Message from ${msg.source}${chanSuffix}: ${msg.content}`;
          messages.push({ role: 'user', content });
          await think('observe', `Received intercom message from ${msg.source}${chanSuffix}`, iteration);
        }

        // Context window management — compact before each LLM call
        if (this.contextManager.isNearLimit(messages)) {
          const compaction = this.contextManager.compact(messages);
          await think('reflect', compaction.reflectMessage, iteration);
        }

        log('debug', `Iteration ${iteration}/${MAX_ITERATIONS} — messages=${messages.length} approxTokens=${this.contextManager.countMessageTokens(messages)}`);

        let response: LLMResponse;
        if (hasTools) {
          response = await this.llm.chat(messages, this.toolDefs, this.manifest.model.temperature);
        } else {
          response = await this.llm.chat(messages, undefined, this.manifest.model.temperature);
        }

        // Accumulate usage
        if (response.usage) {
          totalPromptTokens += response.usage.promptTokens;
          totalCompletionTokens += response.usage.completionTokens;
          log('debug', `Iteration ${iteration} usage: prompt=${response.usage.promptTokens} completion=${response.usage.completionTokens}`);
        }

        // Emit chain-of-thought reasoning if present (e.g. Qwen / DeepSeek)
        if (response.reasoning) {
          await this.centrifugo.publishThought('observe', response.reasoning, iteration);
        }

        if (response.toolCalls && response.toolCalls.length > 0) {
          // ── Tool Call Phase ────────────────────────────────────────────────

          // Duplicate-call loop guard
          const sig = JSON.stringify(response.toolCalls.map((tc) => ({
            name: tc.function.name,
            args: tc.function.arguments,
          })));
          if (sig === lastToolCallSignature) {
            log('warn', `Duplicate tool call detected — breaking loop to prevent infinite repetition`);
            await think('reflect', 'Detected duplicate tool call — aborting loop to prevent infinite repetition', iteration);
            return {
              taskId,
              result: null,
              error: 'duplicate_tool_call',
              usage: { promptTokens: totalPromptTokens, completionTokens: totalCompletionTokens, totalTokens: totalPromptTokens + totalCompletionTokens },
              thoughtStream,
              exitReason: 'error',
            };
          }
          lastToolCallSignature = sig;

          // Emit act thoughts for each tool call (sanitized args)
          for (const tc of response.toolCalls) {
            let parsedArgs: Record<string, unknown> = {};
            try { parsedArgs = JSON.parse(tc.function.arguments || '{}') as Record<string, unknown>; } catch { /* ignore */ }
            const sanitized = sanitizeArgs(parsedArgs);
            await think('act', `Calling tool: ${tc.function.name}(${JSON.stringify(sanitized)})`, iteration, {
              toolName: tc.function.name,
              toolArgs: sanitized,
            });
          }

          // Add assistant turn with tool_calls
          messages.push({
            role: 'assistant',
            content: response.content || '',
            tool_calls: response.toolCalls,
          });

          // Execute tools and add results
          const toolResults = await this.tools.executeToolCalls(response.toolCalls);
          for (const result of toolResults) {
            // Pre-truncate tool output before adding to history
            result.content = this.contextManager.truncateToolOutput(result.content);
            messages.push(result);

            const preview = result.content.length > 200
              ? result.content.substring(0, 200) + '...'
              : result.content;
            await think('reflect', `Tool result: ${preview}`, iteration);
          }

          continue;
        }

        // ── Text Response ──────────────────────────────────────────────────
        // Reasoning models (Qwen3, DeepSeek-R1) may produce reasoning_content
        // but empty content. Use reasoning as fallback so the response isn't lost.
        const reply = response.content || response.reasoning || 'No response generated.';
        await think('reflect', `Completed task after ${iteration} iteration(s)`, iteration);

        // Stream response tokens — use the caller-provided taskId so the
        // web frontend can subscribe to the correct Centrifugo channel.
        await this.streamResponse(reply, taskId);

        log('info', `ReasoningLoop complete after ${iteration} iteration(s) — ${reply.length} chars`);

        return {
          taskId,
          result: reply,
          usage: {
            promptTokens: totalPromptTokens,
            completionTokens: totalCompletionTokens,
            totalTokens: totalPromptTokens + totalCompletionTokens,
          },
          thoughtStream,
          exitReason: 'success',
        };
      }

      // ── Max iterations reached ──────────────────────────────────────────────
      log('warn', `ReasoningLoop hit max iterations (${MAX_ITERATIONS})`);
      await think('reflect', `Reached maximum iterations (${MAX_ITERATIONS}) — stopping`, MAX_ITERATIONS);

      return {
        taskId,
        result: null,
        error: 'max_iterations_exceeded',
        usage: { promptTokens: totalPromptTokens, completionTokens: totalCompletionTokens, totalTokens: totalPromptTokens + totalCompletionTokens },
        thoughtStream,
        exitReason: 'max_iterations_exceeded',
      };

    } catch (err) {
      const errMsg = err instanceof Error ? err.message : String(err);
      log('error', `ReasoningLoop error: ${errMsg}`);
      await think('reflect', `Error: ${errMsg}`, 0);

      let exitReason: TaskOutput['exitReason'] = 'error';
      if (err instanceof BudgetExceededError) exitReason = 'budget_exceeded';
      else if (err instanceof ProviderUnavailableError) exitReason = 'provider_unavailable';

      return {
        taskId,
        result: null,
        error: errMsg,
        usage: { promptTokens: totalPromptTokens, completionTokens: totalCompletionTokens, totalTokens: totalPromptTokens + totalCompletionTokens },
        thoughtStream,
        exitReason,
      };
    }
  }

  /** Stream the final response text to Centrifugo in chunks. */
  private async streamResponse(text: string, messageId: string): Promise<void> {
    const chunkSize = 20;
    for (let i = 0; i < text.length; i += chunkSize) {
      const chunk = text.substring(i, i + chunkSize);
      await this.centrifugo.publishStreamToken(messageId, chunk, false);
    }
    await this.centrifugo.publishStreamToken(messageId, '', true);
  }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

const SECRET_ARG_KEYS = new Set(['token', 'key', 'secret', 'password', 'api_key', 'apikey', 'auth', 'credential']);

/** Remove secret-looking values from tool arguments before publishing. */
function sanitizeArgs(args: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(args)) {
    if (SECRET_ARG_KEYS.has(k.toLowerCase())) {
      out[k] = '[REDACTED]';
    } else {
      out[k] = v;
    }
  }
  return out;
}
