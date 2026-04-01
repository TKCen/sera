import type { Pool } from 'pg';
import { SkillInjector } from '../skills/index.js';
import type { ChatMessage } from './LlmRouter.js';
import { Orchestrator } from '../agents/index.js';
import { AgentFactory } from '../agents/index.js';
import { IdentityService } from '../agents/index.js';
import { EmbeddingService } from '../services/embedding.service.js';
import { VectorService } from '../services/vector.service.js';
import type { MemoryNamespace, SearchFilter } from '../services/vector.service.js';
import { Logger } from '../lib/logger.js';

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

/** Structured event emitted during context assembly for debugging/introspection. */
export interface ContextAssemblyEvent {
  stage: string;
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
      emit({ stage: 'assembly.skipped', detail: { reason: 'manifest not found', agentId } });
      return messages;
    }

    // Fetch instance for circle inheritance and metadata
    const instance = await AgentFactory.getInstance(agentId);

    emit({
      stage: 'assembly.started',
      detail: {
        agentId,
        agentName: manifest.metadata.name,
        hasMemoryConfig: !!(manifest.memory || manifest.spec?.memory),
        skillCount: (manifest.skills ?? []).length,
        messageCount: messages.length,
        instanceCircleId: instance?.circle_id ?? null,
      },
    });

    const lastUserMessage = [...messages].reverse().find((m) => m.role === 'user');
    const currentMessage = lastUserMessage?.content ?? '';

    // ── 1. Generate rich base prompt (replaces Runtime's bare fallback) ──────
    // IdentityService.generateStreamingSystemPrompt includes: identity, response
    // format, stability guidelines, subagents, circle context, and tier info.
    // The circle constitution is embedded as ## Project Context in the prompt.
    const circleContext = await this.loadCircleConstitution(instance?.circle_id);
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
      stage: 'skills.injected',
      detail: {
        skillNames: manifest.skills ?? [],
        skillPackages: manifest.skillPackages ?? [],
        circleId: instance?.circle_id ?? null,
        promptLengthChars: skillsPrompt.length,
        promptSource: 'IdentityService.generateStreamingSystemPrompt',
      },
      durationMs: Date.now() - skillsStart,
    });

    // ── 3. RAG memory retrieval (only if agent has memory config) ────────────
    const hasMemory = !!(manifest.memory || manifest.spec?.memory);
    const memoryContext = hasMemory
      ? await this.retrieveMemoryContext(agentId, manifest, currentMessage, emit, instance)
      : null;

    if (!hasMemory) {
      emit({
        stage: 'memory.skipped',
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

    emit({
      stage: 'context.token_budget',
      detail: {
        totalBudget: contextWindow,
        contextWindow,
        systemPromptTokens,
        skillTokens,
        memoryTokens,
        historyTokens,
        model: modelName,
      },
    });

    emit({
      stage: 'assembly.completed',
      detail: {
        finalPromptLengthChars: newSystemContent.length,
        estimatedTokens: Math.ceil(newSystemContent.length / 4),
        memoryInjected: !!memoryContext,
        promptSource: 'IdentityService.generateStreamingSystemPrompt',
      },
      durationMs: Date.now() - assemblyStart,
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
    manifest: import('../agents/index.js').AgentManifest,
    currentMessage: string,
    emit: ContextEventListener,
    instance?: import('../agents/index.js').AgentInstance | null
  ): Promise<string | null> {
    if (!this.embeddingService.isAvailable()) {
      emit({
        stage: 'memory.skipped',
        detail: { reason: 'embedding service unavailable' },
      });
      logger.debug(`ContextAssembler: embedding unavailable for agent ${agentId}, skipping RAG`);
      return null;
    }
    if (!currentMessage.trim()) {
      emit({
        stage: 'memory.skipped',
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
        stage: 'memory.skipped',
        detail: { reason: 'embedding failed', error: (err as Error).message },
      });
      logger.debug('ContextAssembler: failed to embed message, skipping RAG', err);
      return null;
    }

    const filter: SearchFilter = {};
    let results;
    try {
      results = await this.vectorService.search(namespaces, queryVector, DEFAULT_TOP_K, filter);
    } catch (err) {
      emit({
        stage: 'memory.skipped',
        detail: { reason: 'vector search failed', error: (err as Error).message },
      });
      logger.debug('ContextAssembler: vector search failed, skipping RAG', err);
      return null;
    }

    if (results.length === 0) {
      emit({
        stage: 'memory.retrieved',
        detail: {
          namespaces,
          blockCount: 0,
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
    let charCount = 0;

    for (const r of results) {
      const content = (r.payload.content as string | undefined) ?? '';
      const blockXml = `<block id="${r.id}" type="${r.payload.type ?? ''}" scope="${r.namespace}" author="${r.payload.agent_id ?? ''}" timestamp="${r.payload.created_at ?? ''}">${content}</block>`;
      if (charCount + blockXml.length > charBudget) break;
      blocks.push(blockXml);
      scores.push(r.score);
      charCount += blockXml.length;
      logger.debug(
        `ContextAssembler: retrieved block ${r.id} from ${r.namespace} (score=${r.score.toFixed(3)})`
      );
    }

    const elapsed = Date.now() - start;

    emit({
      stage: 'memory.retrieved',
      detail: {
        namespaces,
        searchResultCount: results.length,
        injectedBlockCount: blocks.length,
        charBudget,
        charsUsed: charCount,
        topK: DEFAULT_TOP_K,
        scores: scores.map((s) => Math.round(s * 1000) / 1000),
      },
      durationMs: elapsed,
    });

    if (blocks.length === 0) return null;

    if (elapsed > 200) {
      logger.warn(`ContextAssembler: memory retrieval took ${elapsed}ms (>200ms budget)`);
    }

    return `<memory>\n${blocks.join('\n')}\n</memory>`;
  }
}
