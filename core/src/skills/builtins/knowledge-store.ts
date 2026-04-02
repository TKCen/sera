/**
 * Built-in skill: knowledge-store (Epic 8, Story 8.5)
 *
 * Stores a knowledge block to personal, circle, or global scope.
 */

import type { SkillDefinition } from '../types.js';
import { KNOWLEDGE_BLOCK_TYPES } from '../../memory/blocks/scoped-types.js';
import type {
  KnowledgeBlockType,
  MemoryScope,
  Importance,
} from '../../memory/blocks/scoped-types.js';
import { ScopedMemoryBlockStore } from '../../memory/blocks/ScopedMemoryBlockStore.js';
import { EmbeddingService } from '../../services/embedding.service.js';
import { VectorService } from '../../services/vector.service.js';
import type { MemoryNamespace } from '../../services/vector.service.js';
import { KnowledgeGitService } from '../../memory/KnowledgeGitService.js';
import { MemoryAnalyst } from '../../memory/MemoryAnalyst.js';
import type { LlmRouter } from '../../llm/LlmRouter.js';
import { AuditService } from '../../audit/AuditService.js';
import { Logger } from '../../lib/logger.js';

const logger = new Logger('knowledge-store');

const MEMORY_ROOT = process.env.MEMORY_PATH ?? '/memory';

// Rate limiting: 10 writes/min per agent across all scopes
const writeTimestamps = new Map<string, number[]>();
function checkRateLimit(agentId: string): boolean {
  const now = Date.now();
  const ts = writeTimestamps.get(agentId) ?? [];
  const recent = ts.filter((t) => now - t < 60_000);
  recent.push(now);
  writeTimestamps.set(agentId, recent);
  return recent.length <= 10;
}

const embeddingService = EmbeddingService.getInstance();

export function createKnowledgeStoreSkill(): SkillDefinition {
  return {
    id: 'knowledge-store',
    description: 'Store a knowledge entry to personal, circle, or global memory.',
    source: 'builtin',
    parameters: [
      {
        name: 'content',
        type: 'string',
        description: 'Content of the knowledge block (Markdown)',
        required: true,
      },
      {
        name: 'type',
        type: 'string',
        description: `Memory block type: ${KNOWLEDGE_BLOCK_TYPES.join(', ')}`,
        required: true,
      },
      {
        name: 'scope',
        type: 'string',
        description: "Scope: 'personal' (default), 'circle', or 'global'",
        required: false,
      },
      { name: 'tags', type: 'array', description: 'Optional tag list', required: false },
      { name: 'title', type: 'string', description: 'Optional title', required: false },
      {
        name: 'importance',
        type: 'number',
        description: 'Importance 1-5 (default 3)',
        required: false,
      },
      {
        name: 'circleId',
        type: 'string',
        description: 'Required when scope=circle and agent is in multiple circles',
        required: false,
      },
    ],
    handler: async (params, context) => {
      const content = params['content'];
      if (typeof content !== 'string' || !content.trim()) {
        return { success: false, error: '"content" is required' };
      }

      const rawType = params['type'];
      if (
        typeof rawType !== 'string' ||
        !KNOWLEDGE_BLOCK_TYPES.includes(rawType as KnowledgeBlockType)
      ) {
        return {
          success: false,
          error: `"type" must be one of: ${KNOWLEDGE_BLOCK_TYPES.join(', ')}`,
        };
      }
      const type = rawType as KnowledgeBlockType;

      const scope: MemoryScope =
        typeof params['scope'] === 'string' &&
        ['personal', 'circle', 'global'].includes(params['scope'])
          ? (params['scope'] as MemoryScope)
          : 'personal';

      const agentId = context.agentInstanceId ?? context.agentName;

      if (!checkRateLimit(agentId)) {
        return { success: false, error: 'Rate limit exceeded: max 10 writes per minute' };
      }

      const tags = Array.isArray(params['tags']) ? (params['tags'] as string[]) : [];
      const title = typeof params['title'] === 'string' ? params['title'] : undefined;

      // Keep original params for use in processFact
      (context as any).params = params;
      const importanceRaw = typeof params['importance'] === 'number' ? params['importance'] : 3;
      const importance = Math.max(1, Math.min(5, Math.round(importanceRaw))) as Importance;

      // ── Analysis Step ───────────────────────────────────────────────────
      const analyzeOnSave = context.manifest?.memory?.analyzeOnSave || false;
      const router = (context as any).router as LlmRouter | undefined;
      const analyst = router ? new MemoryAnalyst(router) : null;
      let finalFacts: Array<{
        content: string;
        title?: string | undefined;
        tags: string[];
        importance: Importance;
        scope: MemoryScope;
      }> = [
        {
          content,
          title,
          tags,
          importance,
          scope,
        },
      ];

      if (analyzeOnSave && analyst) {
        try {
          const modelName =
            context.manifest?.spec?.model?.name ?? context.manifest?.model?.name ?? 'default';
          const config = analyst.router.getRegistry().resolve(modelName);
          const analystModel = config?.contextCompactionModel || modelName;
          const analysis = await analyst.analyze(content, analystModel);

          if (analysis.facts.length > 0) {
            finalFacts = analysis.facts.map((f) => ({
              content: f.content,
              title: f.title || undefined,
              tags: [...new Set([...tags, ...f.tags])],
              importance: params['importance'] !== undefined ? importance : f.importance,
              scope: params['scope'] !== undefined ? scope : f.scope,
            }));
          }
        } catch (err) {
          logger.warn('Memory analysis failed, proceeding with original content:', err);
        }
      }

      const results = [];
      for (const fact of finalFacts) {
        const result = await processFact(fact, context, agentId);
        results.push(result);
      }

      if (results.length === 1) {
        return results[0]!;
      }

      const allSuccess = results.every((r) => r.success);
      if (allSuccess) {
        return {
          success: true,
          data: {
            ids: results.map((r) => r.data?.id),
            scopes: results.map((r) => r.data?.scope),
            count: results.length,
          },
        };
      } else {
        return {
          success: false,
          error: 'Some facts failed to store',
          details: results.map((r) => (r.success ? 'success' : r.error)),
        };
      }
    },
  };
}

async function processFact(
  fact: {
    content: string;
    title?: string | undefined;
    tags: string[];
    importance: Importance;
    scope: MemoryScope;
  },
  context: any,
  agentId: string
): Promise<{ success: boolean; data?: any; error?: string }> {
  const { content, title, tags, importance, scope } = fact;
  const embeddingService = EmbeddingService.getInstance();
  const type = (context.params?.['type'] as KnowledgeBlockType) || 'memory';

  // ── Personal scope ──────────────────────────────────────────────────
  if (scope === 'personal') {
    const store = new ScopedMemoryBlockStore(MEMORY_ROOT);
    const block = await store.write({
      content,
      type,
      agentId,
      tags,
      importance,
      ...(title ? { title } : {}),
    });

    if (embeddingService.isAvailable()) {
      try {
        const vectorService = new VectorService('_ks_unused');
        const namespace: MemoryNamespace = `personal:${agentId}`;
        const vector = await embeddingService.embed(`${block.title}\n${block.content}`);
        await vectorService.upsert(block.id, namespace, vector, {
          agent_id: agentId,
          created_at: block.timestamp,
          tags: block.tags,
          type: block.type,
          title: block.title,
          content: block.content,
          importance: block.importance,
          namespace,
        });
      } catch (err) {
        logger.warn(`Failed to index personal block ${block.id}:`, err);
      }
    }

    await recordAudit(agentId, 'knowledge.committed', { blockId: block.id, type, scope });
    return { success: true, data: { id: block.id, scope, success: true } };
  }

  // ── Circle scope ────────────────────────────────────────────────────
  if (scope === 'circle') {
    const capabilities: string[] = context.manifest?.capabilities ?? [];
    if (
      !capabilities.includes('knowledgeWrite:circle') &&
      !capabilities.includes('knowledgeWrite:merge-without-approval')
    ) {
      return {
        success: false,
        error: 'Insufficient capability: knowledgeWrite:circle required',
      };
    }

    const circleId =
      typeof context.params?.['circleId'] === 'string' && context.params['circleId']
        ? context.params['circleId']
        : context.manifest?.metadata?.circle;

    if (!circleId) {
      return { success: false, error: 'No circleId available — agent is not a circle member' };
    }

    const gitService = KnowledgeGitService.getInstance();
    const { block } = await gitService.write(circleId, agentId, context.agentName, {
      content,
      type,
      agentId,
      tags,
      importance,
      ...(title ? { title } : {}),
    });

    const canAutoMerge = capabilities.includes('knowledgeWrite:merge-without-approval');
    let pendingMerge = false;
    if (canAutoMerge) {
      try {
        await gitService.autoMerge(circleId, agentId);
      } catch (err) {
        logger.warn(`Auto-merge failed for agent ${agentId} in circle ${circleId}:`, err);
        pendingMerge = true;
      }
    } else {
      await gitService.createMergeRequest(circleId, agentId, context.agentName).catch((err) => {
        logger.warn('Failed to create merge request:', err);
      });
      pendingMerge = true;
    }

    await recordAudit(agentId, 'knowledge.committed', {
      blockId: block.id,
      type,
      scope,
      circleId,
    });
    return { success: true, data: { id: block.id, scope, success: true, pendingMerge } };
  }

  // ── Global scope ────────────────────────────────────────────────────
  if (scope === 'global') {
    const capabilities: string[] = context.manifest?.capabilities ?? [];
    if (
      !capabilities.includes('knowledgeWrite:global') &&
      !capabilities.includes('knowledgeWrite:merge-without-approval')
    ) {
      return {
        success: false,
        error: 'Insufficient capability: knowledgeWrite:global required',
      };
    }

    const gitService = KnowledgeGitService.getInstance();
    const { block } = await gitService.write('system', agentId, context.agentName, {
      content,
      type,
      agentId,
      tags,
      importance,
      ...(title ? { title } : {}),
    });

    const canAutoMerge = capabilities.includes('knowledgeWrite:merge-without-approval');
    let pendingMerge = false;
    if (canAutoMerge) {
      try {
        await gitService.autoMerge('system', agentId);
      } catch (err) {
        logger.warn(`Auto-merge failed for agent ${agentId} in system circle:`, err);
        pendingMerge = true;
      }
    } else {
      await gitService.createMergeRequest('system', agentId, context.agentName).catch((err) => {
        logger.warn('Failed to create merge request:', err);
      });
      pendingMerge = true;
    }

    await recordAudit(agentId, 'knowledge.committed', { blockId: block.id, type, scope });
    return { success: true, data: { id: block.id, scope, success: true, pendingMerge } };
  }

  return { success: false, error: `Unknown scope: ${scope as string}` };
}

async function recordAudit(
  agentId: string,
  eventType: string,
  payload: Record<string, unknown>
): Promise<void> {
  try {
    await AuditService.getInstance().record({
      actorType: 'agent',
      actorId: agentId,
      actingContext: null,
      eventType,
      payload,
    });
  } catch (err) {
    logger.warn('Audit record failed:', err);
  }
}
