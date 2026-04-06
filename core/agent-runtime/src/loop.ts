/**
 * ReasoningLoop — the Observe → Plan → Act → Reflect cycle inside the container.
 *
 * Accepts a structured task, runs the LLM + tool loop up to MAX_ITERATIONS,
 * and returns a structured result with usage stats.
 */

import type {
  ILLMClient,
  ChatMessage,
  MessageContentBlock,
  ToolDefinition,
  LLMResponse,
  ThinkingLevel,
} from './llmClient.js';
import {
  BudgetExceededError,
  ProviderUnavailableError,
  ContextOverflowError,
  LLMTimeoutError,
} from './llmClient.js';
import type { IToolExecutor } from './tools/index.js';
import { TOOL_GROUPS } from './tools/definitions.js';
import axios from 'axios';
import type { CentrifugoPublisher, ToolOutputCallback } from './centrifugo.js';
import type { RuntimeManifest } from './manifest.js';
import { generateSystemPrompt } from './manifest.js';
import type { CoreMemoryBlock } from './systemPromptBuilder.js';
import { loadBootContext } from './bootContext.js';
import { ContextManager } from './contextManager.js';
import { ToolLoopDetector } from './toolLoopDetector.js';
import { log } from './logger.js';

// ── Helpers ───────────────────────────────────────────────────────────────────

function contentAsString(content: string | MessageContentBlock[]): string {
  if (typeof content === 'string') return content;
  return content
    .map((block) => (block.type === 'text' && block.text !== undefined ? block.text : ''))
    .join('');
}

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
  citations?: Array<{ blockId: string; scope: string; relevance: number }>;
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
  exitReason:
    | 'success'
    | 'max_iterations_exceeded'
    | 'budget_exceeded'
    | 'provider_unavailable'
    | 'context_overflow'
    | 'error'
    | 'shutdown';
}

// ── Constants ─────────────────────────────────────────────────────────────────

const MAX_ITERATIONS = 10;
const MAX_OVERFLOW_RETRIES = 3;
const MAX_TIMEOUT_RETRIES = 2;
const MAX_PROVIDER_RETRIES = 1;
const PROVIDER_RETRY_BASE_MS = 2000;
const TIMEOUT_COMPACTION_THRESHOLD = 0.65;
/** Max tools sent per LLM call. Beyond this, remaining tools are deferred and discoverable via tool-search. */
const CORE_TOOL_LIMIT = 12;

interface RetryState {
  overflowCompactionAttempts: number;
  timeoutCompactionAttempts: number;
  providerUnavailableAttempts: number;
  toolResultTruncationAttempted: boolean;
}

// ── Tool group selection ──────────────────────────────────────────────────────

/**
 * Determine which tool groups should be active for a given task string.
 * Always-on: core + memory. Additional groups activated by keyword matching.
 * Manifest-declared skillGroups are always merged in.
 */
function selectActiveGroups(task: string, manifestGroups: string[] = []): Set<string> {
  const active = new Set<string>(['core', 'memory']);

  // Merge manifest-declared groups (skip unknown group names)
  for (const g of manifestGroups) {
    if (!(g in TOOL_GROUPS)) {
      // Unknown group — skip silently to avoid empty gated sets
      continue;
    }
    active.add(g);
  }

  const lower = task.toLowerCase();

  if (/url|fetch|http/.test(lower)) active.add('web');
  if (/exec|run|shell|bash|script|code/.test(lower)) active.add('compute');
  if (/spawn|delegate|subagent/.test(lower)) active.add('orchestration');
  if (/file|read|write|edit|glob|grep/.test(lower)) active.add('filesystem');

  return active;
}

// ── ReasoningLoop ─────────────────────────────────────────────────────────────

export class ReasoningLoop {
  private llm: ILLMClient;
  private tools: IToolExecutor;
  private centrifugo: CentrifugoPublisher;
  private manifest: RuntimeManifest;
  private systemPrompt: string;
  /** Core tools sent on every LLM call (up to CORE_TOOL_LIMIT). */
  private toolDefs: ToolDefinition[];
  /** All tools including deferred ones (for tool-search). */
  private allToolDefs: ToolDefinition[];
  private contextManager: ContextManager;
  private bootContext: string = '';
  private coreMemoryBlocks: CoreMemoryBlock[] = [];

  /** Set to true when SIGTERM received; loop exits after current step. */
  shutdownRequested = false;

  constructor(
    llm: ILLMClient,
    tools: IToolExecutor,
    centrifugo: CentrifugoPublisher,
    manifest: RuntimeManifest
  ) {
    this.llm = llm;
    this.tools = tools;
    this.centrifugo = centrifugo;
    this.manifest = manifest;
    this.allToolDefs = tools.getToolDefinitions(manifest.tools?.allowed);
    this.contextManager = new ContextManager(manifest.model.name);

    const workspacePath = process.env['WORKSPACE_PATH'] || '/workspace';
    this.bootContext = loadBootContext(manifest, workspacePath);

    // Initial system prompt — will be refreshed per task with current time/tools
    this.systemPrompt = this.refreshSystemPrompt();

    // toolDefs will be set per-run in run() via context-aware gating (#535).
    this.toolDefs = this.allToolDefs;
  }

  /**
   * Queue of incoming intercom messages to be injected into the loop.
   */
  private incomingMessages: Array<{ source: string; content: string; channel?: string }> = [];

  /**
   * Inject a message from another agent into the next reasoning step.
   */
  public receiveIncomingMessage(from: string, content: string, channel?: string): void {
    log(
      'info',
      `Received message from ${from}${channel ? ` on ${channel}` : ''}: ${content.substring(0, 50)}...`
    );
    this.incomingMessages.push({ source: from, content, channel });
  }

  /** Refresh core memory blocks from Core API. */
  private async refreshCoreMemory(): Promise<void> {
    const coreUrl = process.env['SERA_CORE_URL'] || 'http://sera-core:3001';
    const token = process.env['SERA_IDENTITY_TOKEN'];
    const agentId = process.env['AGENT_INSTANCE_ID'];

    if (!token || !agentId) return;

    try {
      const res = await axios.get<CoreMemoryBlock[]>(`${coreUrl}/api/memory/${agentId}/core`, {
        headers: { Authorization: `Bearer ${token}` },
      });
      this.coreMemoryBlocks = res.data;
    } catch (err) {
      log(
        'warn',
        `Failed to refresh core memory: ${err instanceof Error ? err.message : String(err)}`
      );
    }
  }

  /** Refresh the system prompt with current runtime state. */
  private refreshSystemPrompt(): string {
    const availableAgents = this.manifest.subagents?.allowed?.map((sa) => ({
      name: sa.role, // In this context, name/role are interchangeable for fallback
      role: sa.role,
    }));

    return generateSystemPrompt(this.manifest, {
      tools: this.toolDefs,
      timezone: process.env['TZ'] || 'UTC',
      circleName: this.manifest.metadata.circle,
      availableAgents,
      coreMemoryBlocks: this.coreMemoryBlocks,
      // Budget: 15% of context window per requirement
      tokenBudget: Math.floor(this.contextManager.getContextWindow() * 0.15),
    });
  }

  /**
   * Run the reasoning loop for the given task.
   */
  async run(input: TaskInput): Promise<TaskOutput> {
    const { taskId, task, context, history = [] } = input;

    // Fetch core memory before starting the loop
    await this.refreshCoreMemory();

    // Context-aware tool gating: select groups active for this task, then filter allToolDefs.
    // Always-on groups (core, memory) + keyword-triggered + manifest skillGroups.
    const activeGroups = selectActiveGroups(task, this.manifest.tools?.skillGroups);
    const activeToolNames = new Set<string>();
    for (const [group, names] of Object.entries(TOOL_GROUPS)) {
      if (activeGroups.has(group)) {
        for (const n of names) activeToolNames.add(n);
      }
    }
    const gatedToolDefs = this.allToolDefs.filter((t) => activeToolNames.has(t.function.name));

    // Apply existing CORE_TOOL_LIMIT cap on the gated set
    const manifestCoreTools = this.manifest.tools?.coreTools;
    if (manifestCoreTools) {
      const searchTool = gatedToolDefs.find((t) => t.function.name === 'tool-search');
      this.toolDefs = [
        ...gatedToolDefs.filter((t) => (manifestCoreTools as string[]).includes(t.function.name)),
        ...(searchTool && !(manifestCoreTools as string[]).includes('tool-search')
          ? [searchTool]
          : []),
      ];
    } else if (gatedToolDefs.length > CORE_TOOL_LIMIT) {
      const searchTool = gatedToolDefs.find((t) => t.function.name === 'tool-search');
      const withoutSearch = gatedToolDefs.filter((t) => t.function.name !== 'tool-search');
      this.toolDefs = [
        ...withoutSearch.slice(0, CORE_TOOL_LIMIT),
        ...(searchTool ? [searchTool] : []),
      ];
    } else {
      this.toolDefs = gatedToolDefs;
    }

    // Guard: if gating produced empty toolset but we have tools available, fall back
    if (this.toolDefs.length === 0 && this.allToolDefs.length > 0) {
      log('warn', 'Tool gating produced empty set — falling back to all tools');
      this.toolDefs = this.allToolDefs;
    }

    log(
      'info',
      `Tool gating: groups=[${[...activeGroups].join(',')}] active=${this.toolDefs.length}/${this.allToolDefs.length}`
    );

    // Refresh prompt to get current UTC time, current tool set, and core memory
    this.systemPrompt = this.refreshSystemPrompt();

    const hasTools = this.toolDefs.length > 0;
    const thoughtStream: TaskOutput['thoughtStream'] = [];
    const citations: Array<{ blockId: string; scope: string; relevance: number }> = [];
    const seenCitationIds = new Set<string>();

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
      opts?: {
        toolName?: string;
        toolArgs?: Record<string, unknown>;
        toolCallId?: string;
        anomaly?: boolean;
        internal?: boolean;
      }
    ) => {
      thoughtStream.push({ step, content, iteration, timestamp: new Date().toISOString() });
      await this.centrifugo.publishThought(step, content, iteration, opts);
    };

    // Build initial message array
    const messages: ChatMessage[] = [{ role: 'system', content: this.systemPrompt }];

    if (this.bootContext) {
      messages.push({
        role: 'system',
        content: `The following documents are provided for initial context:\n\n${this.bootContext}`,
        internal: true,
      });
    }

    messages.push(...history);
    messages.push({
      role: 'user',
      content: context ? `${context}\n\n${task}` : task,
    });

    const toolNames = this.toolDefs.map((t) => t.function.name).join(', ') || 'none';
    await think(
      'observe',
      `Received task: "${task.substring(0, 100)}${task.length > 100 ? '...' : ''}"`,
      0
    );
    await think('plan', `Planning approach. Available tools: ${toolNames}`, 0);

    // ── Tool loop detection (per-run) ──────────────────────────────────────
    const loopDetector = new ToolLoopDetector();
    let forceTextNext = false;

    // ── Pre-compaction memory flush (fires at most once per run) ───────
    let memoryFlushFired = false;
    const MEMORY_TOOLS = ['knowledge-store', 'store-memory', 'update-memory'];
    const flushTools = this.allToolDefs.filter((t) => MEMORY_TOOLS.includes(t.function.name));

    // ── Retry state machine (per-run, reset on each invocation) ───────────
    const retryState: RetryState = {
      overflowCompactionAttempts: 0,
      timeoutCompactionAttempts: 0,
      providerUnavailableAttempts: 0,
      toolResultTruncationAttempted: false,
    };

    try {
      let iteration = 0;

      while (iteration < MAX_ITERATIONS) {
        if (this.shutdownRequested) {
          await think('reflect', 'Shutdown requested — stopping reasoning loop', iteration, {
            anomaly: false,
          });
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
          await think(
            'observe',
            `Received intercom message from ${msg.source}${chanSuffix}`,
            iteration
          );
        }

        // Context window management — clear old tool results or compact before each LLM call
        const currentTokens = this.contextManager.countMessageTokens(messages);
        if (currentTokens >= this.contextManager.getClearThreshold()) {
          const clearedCount = this.contextManager.clearOldToolResults(messages, 3);
          if (clearedCount > 0) {
            await think(
              'reflect',
              `Cleared ${clearedCount} old tool result(s) to free space ([cleared — re-read if needed] placeholders used)`,
              iteration
            );
          }
        }

        if (this.contextManager.isNearLimit(messages)) {
          // Pre-compaction memory flush (Story 5.12): allow one turn with only
          // memory tools before compaction.
          if (
            this.contextManager.isMemoryFlushEnabled() &&
            flushTools.length > 0 &&
            !memoryFlushFired
          ) {
            memoryFlushFired = true;
            await think(
              'reflect',
              'Context window approaching limit — triggering memory flush',
              iteration,
              {
                internal: true,
              }
            );
            messages.push({
              role: 'system',
              content:
                'Your context window is about to be compacted. Before this happens, use your memory tools ' +
                'to save any important information from the current conversation that you want to remember. ' +
                'This is your last chance to persist this context.',
              internal: true,
              tokens: this.contextManager.estimateMessageTokens({
                role: 'system',
                content:
                  'Your context window is about to be compacted. Before this happens, use your memory tools ' +
                  'to save any important information from the current conversation that you want to remember. ' +
                  'This is your last chance to persist this context.',
              }),
            });

            // Execute one reasoning turn restricted to memory tools only (30s timeout)
            try {
              const flushResponse = await this.llm.chat(
                messages,
                flushTools,
                this.manifest.model.temperature,
                this.manifest.model.thinkingLevel as ThinkingLevel | undefined,
                30_000
              );

              if (flushResponse.toolCalls && flushResponse.toolCalls.length > 0) {
                messages.push({
                  role: 'assistant',
                  content: flushResponse.content || '',
                  tool_calls: flushResponse.toolCalls,
                  internal: true,
                });

                const toolResults = await this.tools.executeToolCalls(flushResponse.toolCalls);
                let savedCount = 0;
                for (const tr of toolResults) {
                  tr.message.internal = true;
                  messages.push(tr.message);
                  if (
                    MEMORY_TOOLS.includes(tr.toolName) &&
                    !contentAsString(tr.message.content).includes('Error:')
                  ) {
                    savedCount++;
                  }
                  await think(
                    'reflect',
                    `Flush tool result: ${contentAsString(tr.message.content).substring(0, 100)}`,
                    iteration,
                    {
                      internal: true,
                    }
                  );
                }

                log('info', `memory.flush: saved ${savedCount} block(s) before compaction`);
              } else {
                messages.push({
                  role: 'assistant',
                  content: flushResponse.content || '',
                  internal: true,
                });
              }
            } catch (flushErr) {
              log(
                'warn',
                `Memory flush failed: ${flushErr instanceof Error ? flushErr.message : String(flushErr)}`
              );
            }
          } else {
            // Flush already fired or no memory tools — proceed with compaction
            await think('reflect', 'compaction.started', iteration);
            const compaction = await this.contextManager.compact(messages, this.llm);
            const event = compaction.isFallback ? 'compaction.fallback' : 'compaction.completed';
            await think('reflect', `${event}: ${compaction.reflectMessage}`, iteration, {
              anomaly: compaction.isFallback,
            });
          }
        }

        log(
          'debug',
          `Iteration ${iteration}/${MAX_ITERATIONS} — messages=${messages.length} approxTokens=${this.contextManager.countMessageTokens(messages)}`
        );

        // ── LLM call with retry recovery ──────────────────────────────────
        let response: LLMResponse;
        try {
          const useTools = hasTools && !forceTextNext;
          forceTextNext = false; // reset after use
          if (useTools) {
            response = await this.llm.chat(
              messages,
              this.toolDefs,
              this.manifest.model.temperature,
              this.manifest.model.thinkingLevel as ThinkingLevel | undefined
            );
          } else {
            response = await this.llm.chat(
              messages,
              undefined,
              this.manifest.model.temperature,
              this.manifest.model.thinkingLevel as ThinkingLevel | undefined
            );
          }
        } catch (llmErr) {
          // ── Context overflow recovery ────────────────────────────────────
          if (llmErr instanceof ContextOverflowError) {
            if (retryState.overflowCompactionAttempts < MAX_OVERFLOW_RETRIES) {
              retryState.overflowCompactionAttempts++;
              const attempt = retryState.overflowCompactionAttempts;

              await think('reflect', 'compaction.started (aggressive)', iteration);
              const compaction = await this.contextManager.aggressiveCompact(messages, this.llm);
              const event = compaction.isFallback ? 'compaction.fallback' : 'compaction.completed';
              await think(
                'reflect',
                `${event}: Context overflow detected (attempt ${attempt}/${MAX_OVERFLOW_RETRIES}) — ${compaction.reflectMessage}`,
                iteration,
                { anomaly: true }
              );

              // On 2nd+ attempt, try one-shot tool result truncation
              if (attempt >= 2 && !retryState.toolResultTruncationAttempted) {
                retryState.toolResultTruncationAttempted = true;
                const truncated = this.contextManager.truncateAllToolResults(messages);
                if (truncated > 0) {
                  await think(
                    'reflect',
                    `Retroactively truncated ${truncated} tool result(s) for overflow recovery`,
                    iteration,
                    { anomaly: true }
                  );
                }
              }

              // Retry — does NOT increment iteration (not forward progress)
              iteration--;
              continue;
            }

            // All overflow retries exhausted
            log(
              'error',
              `Context overflow: all ${MAX_OVERFLOW_RETRIES} compaction retries exhausted`
            );
            await think(
              'reflect',
              `Context overflow unrecoverable after ${MAX_OVERFLOW_RETRIES} compaction attempts`,
              iteration,
              { anomaly: true }
            );
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
            if (
              utilization > TIMEOUT_COMPACTION_THRESHOLD &&
              retryState.timeoutCompactionAttempts < MAX_TIMEOUT_RETRIES
            ) {
              retryState.timeoutCompactionAttempts++;
              const attempt = retryState.timeoutCompactionAttempts;

              await think('reflect', 'compaction.started (aggressive)', iteration);
              const compaction = await this.contextManager.aggressiveCompact(messages, this.llm);
              const event = compaction.isFallback ? 'compaction.fallback' : 'compaction.completed';
              await think(
                'reflect',
                `${event}: LLM timeout with ${(utilization * 100).toFixed(0)}% context utilization (attempt ${attempt}/${MAX_TIMEOUT_RETRIES}) — ${compaction.reflectMessage}`,
                iteration,
                { anomaly: true }
              );

              // Retry — does NOT increment iteration
              iteration--;
              continue;
            }

            // Low utilization or retries exhausted — propagate to outer catch
            throw llmErr;
          }

          // ── Provider unavailable recovery (#584) ────────────────────────
          if (llmErr instanceof ProviderUnavailableError) {
            if (retryState.providerUnavailableAttempts < MAX_PROVIDER_RETRIES) {
              retryState.providerUnavailableAttempts++;
              const attempt = retryState.providerUnavailableAttempts;
              const delayMs = PROVIDER_RETRY_BASE_MS * Math.pow(2, attempt - 1);

              await think(
                'reflect',
                `Provider unavailable (attempt ${attempt}/${MAX_PROVIDER_RETRIES}) — retrying in ${(delayMs / 1000).toFixed(0)}s`,
                iteration,
                { anomaly: true }
              );
              await new Promise((resolve) => setTimeout(resolve, delayMs));

              // Retry — does NOT increment iteration (not forward progress)
              iteration--;
              continue;
            }

            // All provider retries exhausted — propagate to outer catch
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
          log(
            'debug',
            `Iteration ${iteration} usage: prompt=${response.usage.promptTokens} completion=${response.usage.completionTokens} cacheRead=${response.usage.cacheReadTokens} turn=${turnCount}`
          );
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
            try {
              parsedArgs = JSON.parse(tc.function.arguments || '{}') as Record<string, unknown>;
            } catch {
              /* best effort for detection */
            }

            const verdict = loopDetector.record(tc.function.name, parsedArgs);
            if (verdict.detected) {
              await think(
                'reflect',
                `Loop detected (${verdict.kind}): ${verdict.description}`,
                iteration,
                { anomaly: true }
              );

              if (loopDetector.shouldForceTextResponse()) {
                forceTextNext = true;
                await think(
                  'reflect',
                  'Multiple loop warnings issued — forcing text-only response on next iteration',
                  iteration,
                  { anomaly: true }
                );
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
            try {
              parsedArgs = JSON.parse(tc.function.arguments || '{}') as Record<string, unknown>;
            } catch {
              /* ignore */
            }
            const sanitized = sanitizeArgs(parsedArgs);
            await think(
              'act',
              `Calling tool: ${tc.function.name}(${JSON.stringify(sanitized)})`,
              iteration,
              {
                toolName: tc.function.name,
                toolArgs: sanitized,
                toolCallId: tc.id,
              }
            );
          }

          // Add assistant turn with tool_calls
          messages.push({
            role: 'assistant',
            content: response.content || '',
            tool_calls: response.toolCalls,
          });

          // Execute tools and add results

          const onToolOutput: ToolOutputCallback = (event) => {
            this.centrifugo.publishToolOutput(event, taskId).catch((err) => {
              log(
                'warn',
                `Failed to publish tool output: ${err instanceof Error ? err.message : String(err)}`
              );
            });
          };
          const toolResults = await this.tools.executeToolCalls(response.toolCalls, onToolOutput);
          for (const result of toolResults) {
            // Handle image-view vision request (#NEW)
            if (
              result.toolName === 'image-view' &&
              contentAsString(result.message.content).includes('"vision_request"')
            ) {
              try {
                const parsed = JSON.parse(contentAsString(result.message.content));
                if (parsed.__type === 'vision_request') {
                  const hasVision =
                    this.manifest.model.name.toLowerCase().includes('vision') ||
                    this.manifest.model.name.toLowerCase().includes('gpt-4o') ||
                    this.manifest.model.name.toLowerCase().includes('claude-3');

                  if (!hasVision) {
                    result.message.content =
                      'Error: Current model does not support vision/image analysis. Please switch to a vision-capable model.';
                  } else {
                    // Inject a vision block in the NEXT turn's user message
                    // We transform the tool result into a directive for the loop
                    // But for now, we'll follow the requirement to return it as a vision content block.
                    // Actually, the requirement says "Include as a vision content block in the next LLM call"
                    // and "The image content block should be added as a user message with image_url type"

                    // We can't easily add a user message *between* tool results and the next LLM call in this loop
                    // without modifying how messages are handled.
                    // Strategy: Replace the tool result content with a placeholder,
                    // and push a NEW user message with the image block.

                    result.message.content = `[Image "${parsed.path}" loaded and sent to model for analysis]`;
                    messages.push(result.message);

                    const visionPrompt = parsed.prompt || 'Analyze this image.';
                    messages.push({
                      role: 'user',
                      content: [
                        { type: 'text', text: visionPrompt },
                        { type: 'image_url', image_url: { url: parsed.image_url } },
                      ],
                    });

                    await think(
                      'reflect',
                      `Image "${parsed.path}" loaded and injected into conversation`,
                      iteration
                    );
                    continue; // Skip standard message.push below
                  }
                }
              } catch (e) {
                // Not a valid vision request JSON, treat as regular text
              }
            }

            // 1. Per-result absolute cap (TOOL_OUTPUT_MAX_TOKENS)
            result.message.content = this.contextManager.truncateToolOutput(
              contentAsString(result.message.content)
            );

            // 2. Context-aware budget guard — truncate further if remaining budget is tight
            const budgetCheck = this.contextManager.truncateToContextBudget(
              contentAsString(result.message.content),
              messages
            );
            if (budgetCheck.compactionNeeded) {
              await think('reflect', 'compaction.started (pre-result)', iteration);
              const compaction = await this.contextManager.compact(messages, this.llm);
              const event = compaction.isFallback ? 'compaction.fallback' : 'compaction.completed';
              await think(
                'reflect',
                `${event} (pre-result): ${compaction.reflectMessage}`,
                iteration
              );
              const recheck = this.contextManager.truncateToContextBudget(
                result.message.content,
                messages
              );
              result.message.content = recheck.content;
            } else {
              result.message.content = budgetCheck.content;
            }

            messages.push(result.message);

            // Refresh core memory if a core memory tool was called
            if (
              result.toolName === 'core_memory_append' ||
              result.toolName === 'core_memory_replace'
            ) {
              if (!result.message.content.includes('Error:')) {
                await this.refreshCoreMemory();
                this.systemPrompt = this.refreshSystemPrompt();
                // Update the system prompt in the message array
                if (messages.length > 0 && messages[0].role === 'system') {
                  messages[0].content = this.systemPrompt;
                }
              }
            }

            // Emit reflect thought for argument repair
            if (result.argRepaired) {
              await think(
                'reflect',
                `Repaired malformed tool arguments for ${result.toolName} (strategy: ${result.repairStrategy})`,
                iteration,
                { anomaly: true }
              );
            }

            const preview =
              result.message.content.length > 200
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

        // Strip explicit citations from reply if citations mode is 'brief'
        let finalReply = reply;
        const citationsMode = this.manifest.memory?.citations ?? 'off';
        if (citationsMode === 'brief') {
          finalReply = reply.replace(/\[from:\s*[^\]]+\]/g, '').trim();
          // Clean up multiple spaces left by removal
          finalReply = finalReply.replace(/\s{2,}/g, ' ');
        }

        // Accumulate citations from the final text-producing turn
        if (response.citations && citationsMode !== 'off') {
          for (const c of response.citations) {
            if (!seenCitationIds.has(c.blockId)) {
              citations.push(c);
              seenCitationIds.add(c.blockId);
            }
          }
        }

        await think('reflect', `Completed task after ${iteration} iteration(s)`, iteration);

        // Stream response tokens — use the caller-provided taskId so the
        // web frontend can subscribe to the correct Centrifugo channel.
        await this.streamResponse(finalReply, taskId);

        log(
          'info',
          `ReasoningLoop complete after ${iteration} iteration(s) — ${finalReply.length} chars`
        );

        return {
          taskId,
          result: finalReply,
          usage: buildUsage(),
          thoughtStream,
          citations: citations.length > 0 ? citations : undefined,
          exitReason: 'success',
        };
      }

      // ── Max iterations reached ──────────────────────────────────────────────
      log('warn', `ReasoningLoop hit max iterations (${MAX_ITERATIONS})`);
      await think(
        'reflect',
        `Reached maximum iterations (${MAX_ITERATIONS}) — stopping`,
        MAX_ITERATIONS
      );

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

      // Publish error to Centrifugo so the web UI stops the spinner (#553)
      try {
        await this.centrifugo.publishStreamError(taskId, errMsg);
      } catch (pubErr) {
        log(
          'warn',
          `Failed to publish stream error: ${pubErr instanceof Error ? pubErr.message : String(pubErr)}`
        );
      }

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

const SECRET_ARG_KEYS = new Set([
  'token',
  'key',
  'secret',
  'password',
  'api_key',
  'apikey',
  'auth',
  'credential',
]);

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
