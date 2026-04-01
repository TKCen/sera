/**
 * ReasoningLoop — the Observe → Plan → Act → Reflect cycle inside the container.
 *
 * Accepts a structured task, runs the LLM + tool loop up to MAX_ITERATIONS,
 * and returns a structured result with usage stats.
 */

import type { LLMClient, ChatMessage, ToolDefinition, LLMResponse, ThinkingLevel } from './llmClient.js';
import { BudgetExceededError, ProviderUnavailableError, ContextOverflowError, LLMTimeoutError } from './llmClient.js';
import type { RuntimeToolExecutor } from './tools/index.js';
import type { CentrifugoPublisher } from './centrifugo.js';
import type { RuntimeManifest } from './manifest.js';
import { generateSystemPrompt } from './manifest.js';
import { ContextManager } from './contextManager.js';
import { ToolLoopDetector } from './toolLoopDetector.js';
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
    cacheCreationTokens: number;
    cacheReadTokens: number;
    totalTokens: number;
    turns: number;
  };
  /** Ordered list of thought events for Story 5.9. */
  thoughtStream: Array<{ step: string; content: string; iteration: number; timestamp: string }>;
  exitReason: 'success' | 'max_iterations_exceeded' | 'budget_exceeded' | 'provider_unavailable' | 'context_overflow' | 'error' | 'shutdown';
}

// ── Constants ─────────────────────────────────────────────────────────────────

const MAX_ITERATIONS = 10;
const MAX_OVERFLOW_RETRIES = 3;
const MAX_TIMEOUT_RETRIES = 2;
const TIMEOUT_COMPACTION_THRESHOLD = 0.65;

interface RetryState {
  overflowCompactionAttempts: number;
  timeoutCompactionAttempts: number;
  toolResultTruncationAttempted: boolean;
}

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
    this.toolDefs = tools.getToolDefinitions(manifest.tools?.allowed);
    this.systemPrompt = generateSystemPrompt(manifest);
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

    // ── Usage tracking with cache awareness (#547) ────────────────────────
    let totalPromptTokens = 0;
    let totalCompletionTokens = 0;
    let totalCacheCreationTokens = 0;
    let totalCacheReadTokens = 0;
    let turnCount = 0;

    const buildUsage = () => ({
      promptTokens: totalPromptTokens,
      completionTokens: totalCompletionTokens,
      cacheCreationTokens: totalCacheCreationTokens,
      cacheReadTokens: totalCacheReadTokens,
      totalTokens: totalPromptTokens + totalCompletionTokens,
      turns: turnCount,
    });

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

    // ── Tool loop detection (per-run) ──────────────────────────────────────
    const loopDetector = new ToolLoopDetector();
    let forceTextNext = false;

    // ── Pre-compaction memory save hook (fires at most once per run) ───────
    let preCompactionHookFired = false;
    const hasKnowledgeStore = this.toolDefs.some((t) => t.function.name === 'knowledge-store');

    // ── Retry state machine (per-run, reset on each invocation) ───────────
    const retryState: RetryState = {
      overflowCompactionAttempts: 0,
      timeoutCompactionAttempts: 0,
      toolResultTruncationAttempted: false,
    };

    try {
      let iteration = 0;

      while (iteration < MAX_ITERATIONS) {
        if (this.shutdownRequested) {
          await think('reflect', 'Shutdown requested — stopping reasoning loop', iteration, { anomaly: false });
          return {
            taskId,
            result: null,
            error: 'shutdown',
            usage: buildUsage(),
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
          // Pre-compaction memory save hook (#506): give the agent one iteration
          // to save important context before it's compacted away.
          if (hasKnowledgeStore && !preCompactionHookFired) {
            preCompactionHookFired = true;
            await think('reflect', 'Context window nearly full — injecting save-reminder before compaction', iteration);
            messages.push({
              role: 'system',
              content:
                'IMPORTANT: Your context window is nearly full and compaction will occur shortly. ' +
                'If there is any critical information from this conversation that you want to ' +
                'preserve long-term, call the knowledge-store tool NOW to save it. ' +
                'After this turn, older messages will be dropped.',
            });
            continue; // Let the next iteration handle the save, then compact
          }

          const compaction = this.contextManager.compact(messages);
          await think('reflect', compaction.reflectMessage, iteration);
        }

        log('debug', `Iteration ${iteration}/${MAX_ITERATIONS} — messages=${messages.length} approxTokens=${this.contextManager.countMessageTokens(messages)}`);

        // ── LLM call with retry recovery ──────────────────────────────────
        let response: LLMResponse;
        try {
          const useTools = hasTools && !forceTextNext;
          forceTextNext = false; // reset after use
          if (useTools) {
            response = await this.llm.chat(messages, this.toolDefs, this.manifest.model.temperature, this.manifest.model.thinkingLevel as ThinkingLevel | undefined);
          } else {
            response = await this.llm.chat(messages, undefined, this.manifest.model.temperature, this.manifest.model.thinkingLevel as ThinkingLevel | undefined);
          }
        } catch (llmErr) {
          // ── Context overflow recovery ────────────────────────────────────
          if (llmErr instanceof ContextOverflowError) {
            if (retryState.overflowCompactionAttempts < MAX_OVERFLOW_RETRIES) {
              retryState.overflowCompactionAttempts++;
              const attempt = retryState.overflowCompactionAttempts;

              const compaction = this.contextManager.aggressiveCompact(messages);
              await think('reflect', `Context overflow detected (attempt ${attempt}/${MAX_OVERFLOW_RETRIES}) — ${compaction.reflectMessage}`, iteration, { anomaly: true });

              // On 2nd+ attempt, try one-shot tool result truncation
              if (attempt >= 2 && !retryState.toolResultTruncationAttempted) {
                retryState.toolResultTruncationAttempted = true;
                const truncated = this.contextManager.truncateAllToolResults(messages);
                if (truncated > 0) {
                  await think('reflect', `Retroactively truncated ${truncated} tool result(s) for overflow recovery`, iteration, { anomaly: true });
                }
              }

              // Retry — does NOT increment iteration (not forward progress)
              iteration--;
              continue;
            }

            // All overflow retries exhausted
            log('error', `Context overflow: all ${MAX_OVERFLOW_RETRIES} compaction retries exhausted`);
            await think('reflect', `Context overflow unrecoverable after ${MAX_OVERFLOW_RETRIES} compaction attempts`, iteration, { anomaly: true });
            return {
              taskId,
              result: null,
              error: `Context overflow after ${MAX_OVERFLOW_RETRIES} compaction attempts: ${llmErr.message}`,
              usage: buildUsage(),
              thoughtStream,
              exitReason: 'context_overflow',
            };
          }

          // ── Timeout recovery ────────────────────────────────────────────
          if (llmErr instanceof LLMTimeoutError) {
            const utilization = this.contextManager.getUtilization(messages);
            if (utilization > TIMEOUT_COMPACTION_THRESHOLD && retryState.timeoutCompactionAttempts < MAX_TIMEOUT_RETRIES) {
              retryState.timeoutCompactionAttempts++;
              const attempt = retryState.timeoutCompactionAttempts;

              const compaction = this.contextManager.aggressiveCompact(messages);
              await think('reflect', `LLM timeout with ${(utilization * 100).toFixed(0)}% context utilization (attempt ${attempt}/${MAX_TIMEOUT_RETRIES}) — ${compaction.reflectMessage}`, iteration, { anomaly: true });

              // Retry — does NOT increment iteration
              iteration--;
              continue;
            }

            // Low utilization or retries exhausted — propagate to outer catch
            throw llmErr;
          }

          // All other errors — propagate to outer catch
          throw llmErr;
        }

        // Accumulate usage (including cache tokens)
        if (response.usage) {
          totalPromptTokens += response.usage.promptTokens;
          totalCompletionTokens += response.usage.completionTokens;
          totalCacheCreationTokens += response.usage.cacheCreationTokens;
          totalCacheReadTokens += response.usage.cacheReadTokens;
          turnCount++;
          log('debug', `Iteration ${iteration} usage: prompt=${response.usage.promptTokens} completion=${response.usage.completionTokens} cacheRead=${response.usage.cacheReadTokens} turn=${turnCount}`);
        }

        // Emit chain-of-thought reasoning if present (e.g. Qwen / DeepSeek)
        if (response.reasoning) {
          await this.centrifugo.publishThought('observe', response.reasoning, iteration);
        }

        if (response.toolCalls && response.toolCalls.length > 0) {
          // ── Tool Call Phase ────────────────────────────────────────────────

          // Semantic loop detection — feed each tool call into the detector
          for (const tc of response.toolCalls) {
            let parsedArgs: Record<string, unknown> = {};
            try { parsedArgs = JSON.parse(tc.function.arguments || '{}') as Record<string, unknown>; } catch { /* best effort for detection */ }

            const verdict = loopDetector.record(tc.function.name, parsedArgs);
            if (verdict.detected) {
              await think('reflect', `Loop detected (${verdict.kind}): ${verdict.description}`, iteration, { anomaly: true });

              if (loopDetector.shouldForceTextResponse()) {
                forceTextNext = true;
                await think('reflect', 'Multiple loop warnings issued — forcing text-only response on next iteration', iteration, { anomaly: true });
              } else {
                messages.push({
                  role: 'system',
                  content: `WARNING: Tool loop detected (${verdict.kind}): ${verdict.description}`,
                });
              }
              loopDetector.acknowledgeWarning();
            }
          }

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
            // 1. Per-result absolute cap (TOOL_OUTPUT_MAX_TOKENS)
            result.message.content = this.contextManager.truncateToolOutput(result.message.content);

            // 2. Context-aware budget guard — truncate further if remaining budget is tight
            const budgetCheck = this.contextManager.truncateToContextBudget(result.message.content, messages);
            if (budgetCheck.compactionNeeded) {
              const compaction = this.contextManager.compact(messages);
              await think('reflect', `Pre-result compaction: ${compaction.reflectMessage}`, iteration);
              const recheck = this.contextManager.truncateToContextBudget(result.message.content, messages);
              result.message.content = recheck.content;
            } else {
              result.message.content = budgetCheck.content;
            }

            messages.push(result.message);

            // Emit reflect thought for argument repair
            if (result.argRepaired) {
              await think('reflect', `Repaired malformed tool arguments for ${result.toolName} (strategy: ${result.repairStrategy})`, iteration, { anomaly: true });
            }

            const preview = result.message.content.length > 200
              ? result.message.content.substring(0, 200) + '...'
              : result.message.content;
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
          usage: buildUsage(),
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
        usage: buildUsage(),
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
      else if (err instanceof ContextOverflowError) exitReason = 'context_overflow';

      return {
        taskId,
        result: null,
        error: errMsg,
        usage: buildUsage(),
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
