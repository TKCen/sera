import type { Pool } from 'pg';
import { SkillInjector } from '../skills/SkillInjector.js';
import type { ChatMessage } from './LiteLLMClient.js';
import { Orchestrator } from '../agents/Orchestrator.js';
import { AgentFactory } from '../agents/AgentFactory.js';
import { EmbeddingService } from '../services/embedding.service.js';
import { VectorService } from '../services/vector.service.js';
import type { MemoryNamespace, SearchFilter } from '../services/vector.service.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('ContextAssembler');

// Default token budget for injected memory (characters / 4 ≈ tokens)
const DEFAULT_MEMORY_CHAR_BUDGET = 16_000; // ~4000 tokens
const DEFAULT_TOP_K = 8;

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
  private vectorService = new VectorService('_ctx_assembler_unused');
  private embeddingService = EmbeddingService.getInstance();

  constructor(
    private pool: Pool,
    private orchestrator: Orchestrator
  ) {
    this.skillInjector = new SkillInjector(pool);
  }

  /**
   * Assembles the context for an LLM call:
   * 1. Skill injection
   * 2. RAG — embed current message, search all accessible namespaces,
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

    emit({
      stage: 'assembly.started',
      detail: {
        agentId,
        agentName: manifest.metadata.name,
        hasMemoryConfig: !!manifest.memory,
        skillCount: (manifest.skills ?? []).length,
        messageCount: messages.length,
      },
    });

    // Skip assembly if agent has no memory configuration
    if (!manifest.memory) {
      emit({
        stage: 'assembly.skipped',
        detail: { reason: 'no memory configuration' },
        durationMs: Date.now() - assemblyStart,
      });
      return messages;
    }

    const lastUserMessage = [...messages].reverse().find((m) => m.role === 'user');
    const currentMessage = lastUserMessage?.content ?? '';

    // Fetch instance for circle inheritance and metadata
    const instance = await AgentFactory.getInstance(agentId);

    // 1. Inject skills (and constitution)
    const skillsStart = Date.now();
    const skillsPrompt = await this.skillInjector.inject(
      systemMessage.content ?? '',
      manifest.skills ?? [],
      manifest.skillPackages ?? [],
      currentMessage,
      instance?.circle_id
    );
    emit({
      stage: 'skills.injected',
      detail: {
        skillNames: manifest.skills ?? [],
        skillPackages: manifest.skillPackages ?? [],
        circleId: instance?.circle_id ?? null,
        promptLengthChars: skillsPrompt.length,
      },
      durationMs: Date.now() - skillsStart,
    });

    // 2. RAG memory retrieval
    const memoryContext = await this.retrieveMemoryContext(agentId, manifest, currentMessage, emit);

    const newSystemContent = memoryContext ? `${skillsPrompt}\n\n${memoryContext}` : skillsPrompt;

    emit({
      stage: 'assembly.completed',
      detail: {
        finalPromptLengthChars: newSystemContent.length,
        estimatedTokens: Math.ceil(newSystemContent.length / 4),
        memoryInjected: !!memoryContext,
      },
      durationMs: Date.now() - assemblyStart,
    });

    return messages.map((m) => (m.role === 'system' ? { ...m, content: newSystemContent } : m));
  }

  private async retrieveMemoryContext(
    agentId: string,
    manifest: import('../agents/manifest/types.js').AgentManifest,
    currentMessage: string,
    emit: ContextEventListener
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

    // Circle namespaces
    const primaryCircle = manifest.metadata.circle;
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
