import crypto from 'crypto';
import type { Pool } from 'pg';
import { SkillInjector } from '../skills/SkillInjector.js';
import type { ChatMessage } from './LlmRouter.js';
import { Orchestrator } from '../agents/Orchestrator.js';
import { AgentFactory } from '../agents/AgentFactory.js';
import { IdentityService } from '../agents/identity/IdentityService.js';
import { EmbeddingService } from '../services/embedding.service.js';
import { VectorService } from '../services/vector.service.js';
import type {
  MemoryNamespace,
  SearchFilter,
  HybridSearchConfig,
  SearchResult,
} from '../services/vector.service.js';
import { Logger } from '../lib/logger.js';
import { MemoryBlockStore } from '../memory/blocks/MemoryBlockStore.js';

const logger = new Logger('ContextAssembler');

// Default token budget for injected memory (characters / 4 ≈ tokens)
const DEFAULT_MEMORY_CHAR_BUDGET = 16_000; // ~4000 tokens
const DEFAULT_TOP_K = 8;

// ── Model context window sizes (shared with agent-runtime/contextManager.ts) ──
const MODEL_CONTEXT_WINDOWS: Record<string, number> = {
  'gpt-4o': 128_000,
  'gpt-4o-mini': 128_000,
  'gpt-4-turbo': 128_000,
  'gpt-4': 8_192,
  'gpt-3.5-turbo': 16_385,
  'claude-opus-4': 200_000,
  'claude-sonnet-4': 200_000,
  'claude-haiku-4': 200_000,
  'claude-3-5-sonnet': 200_000,
  'claude-3-5-haiku': 200_000,
  'claude-3-opus': 200_000,
  'qwen2.5-coder-7b': 32_768,
  'qwen2.5-coder-32b': 32_768,
  'qwen3.5-35b-a3b': 131_072,
  'llama3.1:8b': 128_000,
  'llama3.2': 128_000,
};
const DEFAULT_CONTEXT_WINDOW = 128_000;

/** All possible stages for context assembly events. */
export type ContextAssemblyStage =
  | 'context.assembly_started'
  | 'context.circle_constitution_injected'
  | 'context.circle_constitution_skipped'
  | 'context.skills_injected'
  | 'context.memory_retrieved'
  | 'context.memory_skipped'
  | 'context.tools_resolved'
  | 'context.token_budget'
  | 'context.assembly_completed'
  | 'context.assembly_skipped'
  | 'context.assembly_error'
  | 'compaction.skipped'
  | 'compaction.started'
  | 'compaction.summarizing'
  | 'compaction.summarized'
  | 'compaction.completed'
  | 'compaction.fallback';

/** Structured event emitted during context assembly for debugging/introspection. */
export interface ContextAssemblyEvent {
  stage: ContextAssemblyStage;
  detail: Record<string, unknown>;
  durationMs?: number;
}

/** Callback for receiving context assembly events (published as thoughts). */
export type ContextEventListener = (event: ContextAssemblyEvent) => void;

export class ContextAssembler {
  private skillInjector: SkillInjector;
  private vectorService = new VectorService('ctx-assembler');
  private embeddingService = EmbeddingService.getInstance();
  private pool: Pool;

  constructor(
    pool: Pool,
    private orchestrator: Orchestrator
  ) {
    this.pool = pool;
    this.skillInjector = new SkillInjector(pool);
  }

  /**
   * Assembles the context for an LLM call:
   * 1. Replace the Runtime's bare system prompt with the full IdentityService prompt
   * 2. Skill injection
   * 3. RAG — embed current message, search all accessible namespaces,
   *    inject top-K memory blocks into the system prompt.
   *
   * @param onEvent - Optional callback for context assembly events (for thought stream)
   */
  async assemble(
    agentId: string,
    messages: ChatMessage[],
    onEvent?: ContextEventListener
  ): Promise<ChatMessage[]> {
    const assemblyStart = Date.now();
    const emit = onEvent ?? (() => {});

    const systemMessage = messages.find((m) => m.role === 'system');
    if (!systemMessage) return messages;

    const manifest =
      this.orchestrator.getManifestByInstanceId(agentId) || this.orchestrator.getManifest(agentId);

    if (!manifest) {
      emit({
        stage: 'context.assembly_skipped',
        detail: { reason: 'manifest not found', agentId },
      });
      return messages;
    }

    // Fetch instance for circle inheritance and metadata
    const instance = await AgentFactory.getInstance(agentId);

    const lastUserMessage = [...messages].reverse().find((m) => m.role === 'user');
    const currentMessage = lastUserMessage?.content ?? '';
    const messageHash = crypto.createHash('sha256').update(currentMessage).digest('hex');

    emit({
      stage: 'context.assembly_started',
      detail: {
        agentId,
        agentName: manifest.metadata.name,
        messageHash,
        hasMemoryConfig: !!(manifest.memory || manifest.spec?.memory),
        skillCount: (manifest.skills ?? []).length,
        messageCount: messages.length,
        instanceCircleId: instance?.circle_id ?? null,
      },
    });

    // ── 1. Generate rich base prompt (replaces Runtime's bare fallback) ──────
    // IdentityService.generateStreamingSystemPrompt includes: identity, response
    // format, stability guidelines, subagents, circle context, and tier info.
    // The circle constitution is embedded as ## Project Context in the prompt.
    const circleContextStart = Date.now();
    const circleContext = await this.loadCircleConstitution(instance?.circle_id);

    if (circleContext) {
      emit({
        stage: 'context.circle_constitution_injected',
        detail: {
          circleId: instance?.circle_id,
          charCount: circleContext.length,
        },
        durationMs: Date.now() - circleContextStart,
      });
    } else {
      emit({
        stage: 'context.circle_constitution_skipped',
        detail: {
          reason: instance?.circle_id ? 'constitution empty or not found' : 'no circle assigned',
          circleId: instance?.circle_id ?? null,
        },
      });
    }

    const richPrompt = IdentityService.generateStreamingSystemPrompt(manifest, circleContext);

    // ── 2. Inject skills (and constitution if not already in rich prompt) ────
    // Pass null for circleId because the constitution is already injected by
    // IdentityService as ## Project Context — avoids double injection.
    const skillsStart = Date.now();
    const skillsPrompt = await this.skillInjector.inject(
      richPrompt,
      manifest.skills ?? [],
      manifest.skillPackages ?? [],
      currentMessage,
      null // circleId null — constitution already in richPrompt
    );
    emit({
      stage: 'context.skills_injected',
      detail: {
        skillNames: manifest.skills ?? [],
        skillPackages: manifest.skillPackages ?? [],
        circleId: instance?.circle_id ?? null,
        promptLengthChars: skillsPrompt.length,
        tokenCount: Math.ceil(skillsPrompt.length / 4),
        promptSource: 'IdentityService.generateStreamingSystemPrompt',
      },
      durationMs: Date.now() - skillsStart,
    });

    // ── 2.5 Resolve Tools ────────────────────────────────────────────────────
    const allowedTools = manifest.spec?.tools?.allowed ?? manifest.tools?.allowed ?? [];
    const toolExecutor = this.orchestrator.getToolExecutor();
    const availableTools = toolExecutor?.getToolDefinitions(manifest) ?? [];
    const availableToolNames = new Set(availableTools.map((t) => t.function.name));
    const unresolvedNames = allowedTools.filter((name) => !availableToolNames.has(name));

    emit({
      stage: 'context.tools_resolved',
      detail: {
        registeredCount: allowedTools.length,
        resolvedCount: allowedTools.length - unresolvedNames.length,
        unresolvedNames,
      },
    });

    // ── 3. RAG memory retrieval (only if agent has memory config) ────────────
    const hasMemory = !!(manifest.memory || manifest.spec?.memory);
    const memoryContext = hasMemory
      ? await this.retrieveMemoryContext(agentId, manifest, currentMessage, emit, instance)
      : null;

    if (!hasMemory) {
      emit({
        stage: 'context.memory_skipped',
        detail: { reason: 'no memory configuration' },
      });
    }

    const newSystemContent = memoryContext ? `${skillsPrompt}\n\n${memoryContext}` : skillsPrompt;

    // ── 4. Emit token budget visualization for context debugger ─────────────
    const estimateTokens = (text: string) => Math.ceil(text.length / 4);
    const systemPromptTokens = estimateTokens(richPrompt);
    const skillTokens = Math.max(0, estimateTokens(skillsPrompt) - systemPromptTokens);
    const memoryTokens = memoryContext ? estimateTokens(memoryContext) : 0;
    const historyTokens = messages
      .filter((m) => m.role !== 'system')
      .reduce((sum, m) => sum + estimateTokens(m.content ?? ''), 0);

    const modelName = manifest.spec?.model?.name ?? manifest.model?.name ?? 'default';
    const contextWindow = MODEL_CONTEXT_WINDOWS[modelName] ?? DEFAULT_CONTEXT_WINDOW;
    const remaining = Math.max(
      0,
      contextWindow - systemPromptTokens - skillTokens - memoryTokens - historyTokens
    );

    emit({
      stage: 'context.token_budget',
      detail: {
        totalBudget: contextWindow,
        contextWindow,
        systemPromptTokens,
        skillTokens,
        memoryTokens,
        historyTokens,
        remaining,
        model: modelName,
      },
    });

    const totalDurationMs = Date.now() - assemblyStart;
    emit({
      stage: 'context.assembly_completed',
      detail: {
        finalPromptLengthChars: newSystemContent.length,
        finalTokenCount: Math.ceil(newSystemContent.length / 4),
        memoryInjected: !!memoryContext,
        promptSource: 'IdentityService.generateStreamingSystemPrompt',
        totalDurationMs,
      },
      durationMs: totalDurationMs,
    });

    return messages.map((m) => (m.role === 'system' ? { ...m, content: newSystemContent } : m));
  }

  /**
   * Load the circle's constitution text from the database.
   */
  private async loadCircleConstitution(
    circleId: string | null | undefined
  ): Promise<string | undefined> {
    if (!circleId) return undefined;
    try {
      const res = await this.pool.query('SELECT constitution FROM circles WHERE id = $1', [
        circleId,
      ]);
      if (res.rows.length > 0 && res.rows[0].constitution) {
        return res.rows[0].constitution as string;
      }
    } catch (err) {
      logger.warn('Failed to load circle constitution:', err);
    }
    return undefined;
  }

  private async retrieveMemoryContext(
    agentId: string,
    manifest: import('../agents/manifest/types.js').AgentManifest,
    currentMessage: string,
    emit: ContextEventListener,
    instance?: import('../agents/types.js').AgentInstance | null
  ): Promise<string | null> {
    const memoryConfig = manifest.spec?.memory || manifest.memory;
    const searchConfig: HybridSearchConfig = {
      vectorWeight: 0.7,
      textWeight: 0.3,
      minScore: 0.35,
      maxResults: DEFAULT_TOP_K,
      mmr: {
        enabled: true,
        lambda: 0.7,
        candidateMultiplier: 4,
      },
      temporalDecay: {
        enabled: true,
        halfLifeDays: 30,
      },
      ...(memoryConfig?.search || {}),
    };

    if (!this.embeddingService.isAvailable()) {
      emit({
        stage: 'context.memory_skipped',
        detail: { reason: 'embedding service unavailable' },
      });
      logger.debug(`ContextAssembler: embedding unavailable for agent ${agentId}, skipping RAG`);
      return null;
    }
    if (!currentMessage.trim()) {
      emit({
        stage: 'context.memory_skipped',
        detail: { reason: 'empty user message' },
      });
      return null;
    }

    const start = Date.now();

    // Build accessible namespaces
    const namespaces: MemoryNamespace[] = [];

    // Personal namespace — always included
    namespaces.push(`personal:${agentId}`);

    // Circle namespaces — prefer instance's actual circle over template manifest
    const primaryCircle = instance?.circle_id ?? manifest.metadata.circle;
    if (primaryCircle) namespaces.push(`circle:${primaryCircle}`);
    if (manifest.metadata.additionalCircles) {
      for (const c of manifest.metadata.additionalCircles) {
        namespaces.push(`circle:${c}`);
      }
    }

    // Global — included unless manifest explicitly disables it
    // DECISION: all agents have global read access by default
    namespaces.push('global');

    let queryVector: number[];
    try {
      queryVector = await this.embeddingService.embed(currentMessage);
    } catch (err) {
      emit({
        stage: 'context.memory_skipped',
        detail: { reason: 'embedding failed', error: (err as Error).message },
      });
      logger.debug('ContextAssembler: failed to embed message, skipping RAG', err);
      return null;
    }

    const filter: SearchFilter = {};
    let vectorResults: SearchResult[] = [];
    let textResults: SearchResult[] = [];

    const vectorLimit = searchConfig.mmr?.enabled
      ? searchConfig.maxResults * searchConfig.mmr.candidateMultiplier
      : searchConfig.maxResults;

    try {
      vectorResults = await this.vectorService.search(
        namespaces,
        queryVector,
        vectorLimit,
        filter,
        searchConfig.mmr?.enabled
      );
    } catch (err) {
      logger.debug('ContextAssembler: vector search failed', err);
    }

    try {
      // Need a MemoryBlockStore instance to call searchFullText.
      // We can use the one from MemoryManager if accessible, but here we can just create a temporary one
      // or better, implement a static method or a shared service.
      // For now, let's assume we can use a direct DB query or instantiate MemoryBlockStore.
      const rootPath = process.env.MEMORY_PATH || '../memory';
      // The logical namespace doesn't matter for searching across all namespaces,
      // but we need an instance to call the method.
      const store = new MemoryBlockStore(rootPath);
      textResults = (await store.searchFullText(
        currentMessage,
        namespaces.map((ns) => (ns === 'global' ? 'global' : ns)), // Map namespaces if needed
        vectorLimit
      )) as unknown as SearchResult[];
    } catch (err) {
      logger.debug('ContextAssembler: full-text search failed', err);
    }

    let results: SearchResult[] = [];
    try {
      results = await this.vectorService.hybridSearch(
        queryVector,
        vectorResults,
        textResults,
        searchConfig
      );
    } catch (err) {
      emit({
        stage: 'context.memory_skipped',
        detail: { reason: 'hybrid search failed', error: (err as Error).message },
      });
      logger.debug('ContextAssembler: hybrid search failed, skipping RAG', err);
      return null;
    }

    if (results.length === 0) {
      emit({
        stage: 'context.memory_retrieved',
        detail: {
          namespaces,
          blockCount: 0,
          injectedBlockCount: 0,
          tokenCount: 0,
          scores: [],
          topK: DEFAULT_TOP_K,
        },
        durationMs: Date.now() - start,
      });
      return null;
    }

    // Build <memory> block, respect token budget (trim lowest-score blocks first)
    const charBudget = DEFAULT_MEMORY_CHAR_BUDGET;
    const blocks: string[] = [];
    const scores: number[] = [];
    const injectedBlocksMetadata: Array<{
      id: string;
      source: string;
      relevance: number;
      content: string;
    }> = [];
    let charCount = 0;

    for (const r of results) {
      const content = (r.payload.content as string | undefined) ?? '';
      // The desired format is <memory source="..." id="..." relevance="...">...</memory>
      // The old format was <block id="..." type="..." scope="..." author="..." timestamp="...">...</block>
      // We'll migrate to the more compact citation-friendly format.
      const blockXml = `<memory source="${r.namespace}" id="${r.id}" relevance="${r.score.toFixed(3)}">${content}</memory>`;
      if (charCount + blockXml.length > charBudget) break;
      blocks.push(blockXml);
      scores.push(r.score);
      injectedBlocksMetadata.push({
        id: String(r.id),
        source: r.namespace,
        relevance: r.score,
        content,
      });
      charCount += blockXml.length;
      logger.debug(
        `ContextAssembler: retrieved block ${r.id} from ${r.namespace} (score=${r.score.toFixed(3)})`
      );
    }

    const elapsed = Date.now() - start;

    emit({
      stage: 'context.memory_retrieved',
      detail: {
        namespaces,
        blockCount: results.length,
        injectedBlockCount: blocks.length,
        tokenCount: Math.ceil(charCount / 4),
        charBudget,
        charsUsed: charCount,
        topK: DEFAULT_TOP_K,
        scores: scores.map((s) => Math.round(s * 1000) / 1000),
        blocks: injectedBlocksMetadata,
      },
      durationMs: elapsed,
    });

    if (blocks.length === 0) return null;

    if (elapsed > 200) {
      logger.warn(`ContextAssembler: memory retrieval took ${elapsed}ms (>200ms budget)`);
    }

    return `<injected_memory>\n${blocks.join('\n')}\n</injected_memory>`;
  }
}
