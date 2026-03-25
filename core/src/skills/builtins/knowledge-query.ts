/**
 * Built-in skill: knowledge-query (Epic 8, Story 8.6)
 *
 * Semantic search across personal, circle, and global memory scopes.
 */

import type { SkillDefinition } from '../types.js';
import type { MemoryScope } from '../../memory/blocks/scoped-types.js';
import { EmbeddingService } from '../../services/embedding.service.js';
import { VectorService } from '../../services/vector.service.js';
import type { MemoryNamespace, SearchFilter } from '../../services/vector.service.js';
import { Logger } from '../../lib/logger.js';

const logger = new Logger('knowledge-query');

const vectorService = new VectorService('_kq_unused');
const embeddingService = EmbeddingService.getInstance();

const DEFAULT_TOP_K = 10;
const MAX_TOP_K = 30;

export function createKnowledgeQuerySkill(): SkillDefinition {
  return {
    id: 'knowledge-query',
    description:
      'Search memory across personal, circle, and global scopes using semantic similarity.',
    source: 'builtin',
    parameters: [
      { name: 'query', type: 'string', description: 'Search query', required: true },
      {
        name: 'scopes',
        type: 'array',
        description: "Scopes to search: ['personal', 'circle', 'global'] (default: all accessible)",
        required: false,
      },
      {
        name: 'topK',
        type: 'number',
        description: `Max results (default ${DEFAULT_TOP_K}, max ${MAX_TOP_K})`,
        required: false,
      },
      {
        name: 'filter',
        type: 'object',
        description: 'Optional filter: { type?, tags?, since?, author? }',
        required: false,
      },
    ],
    handler: async (params, context) => {
      const queryText = params['query'];
      if (typeof queryText !== 'string' || !queryText.trim()) {
        return { success: false, error: '"query" is required' };
      }

      if (!embeddingService.isAvailable()) {
        return { success: false, error: 'Embedding service unavailable — RAG is disabled' };
      }

      const agentId = context.agentInstanceId ?? context.agentName;
      const topK = Math.min(
        typeof params['topK'] === 'number' ? Math.max(1, params['topK']) : DEFAULT_TOP_K,
        MAX_TOP_K
      );

      // Build requested scopes
      const requestedScopes: MemoryScope[] = Array.isArray(params['scopes'])
        ? (params['scopes'] as string[]).filter((s): s is MemoryScope =>
            ['personal', 'circle', 'global'].includes(s)
          )
        : ['personal', 'circle', 'global'];

      // Build namespaces, enforcing access
      const namespaces: MemoryNamespace[] = [];

      if (requestedScopes.includes('personal')) {
        namespaces.push(`personal:${agentId}`);
      }

      if (requestedScopes.includes('circle')) {
        const primary = context.manifest.metadata.circle;
        const additional = context.manifest.metadata.additionalCircles ?? [];
        const circles = [primary, ...additional].filter(Boolean) as string[];

        if (circles.length === 0) {
          // No circle membership — return early if circle was the only scope
          if (requestedScopes.length === 1) {
            return { success: true, data: { results: [], error: 'not_a_circle_member' } };
          }
          // else fall through with other scopes
        } else {
          for (const c of circles) {
            namespaces.push(`circle:${c}`);
          }
        }
      }

      // Global — always searchable (main branch only)
      if (requestedScopes.includes('global')) {
        namespaces.push('global');
      }

      if (namespaces.length === 0) {
        return { success: true, data: { results: [] } };
      }

      // Build filter
      const filterRaw = params['filter'];
      const filter: SearchFilter = {};
      if (filterRaw && typeof filterRaw === 'object') {
        const f = filterRaw as Record<string, unknown>;
        if (typeof f['type'] === 'string') filter.type = f['type'];
        if (Array.isArray(f['tags'])) filter.tags = f['tags'] as string[];
        if (Array.isArray(f['excludeTags'])) filter.excludeTags = f['excludeTags'] as string[];
        if (typeof f['since'] === 'string') filter.since = f['since'];
        if (typeof f['author'] === 'string') filter.author = f['author'];
        if (typeof f['minImportance'] === 'number') filter.minImportance = f['minImportance'];
      }

      const start = Date.now();
      let queryVector: number[];
      try {
        queryVector = await embeddingService.embed(queryText);
      } catch (err) {
        return {
          success: false,
          error: `Embedding failed: ${err instanceof Error ? err.message : String(err)}`,
        };
      }

      const rawResults = await vectorService.search(namespaces, queryVector, topK, filter);
      const elapsed = Date.now() - start;
      if (elapsed > 300) {
        logger.warn(`knowledge-query: latency ${elapsed}ms exceeds 300ms target`);
      }

      const results = rawResults.map((r) => ({
        id: String(r.id),
        type: r.payload.type ?? '',
        content: r.payload.content ?? '',
        tags: r.payload.tags ?? [],
        relevanceScore: r.score,
        timestamp: r.payload.created_at ?? '',
        scope: r.namespace,
        author: r.payload.agent_id ?? '',
        ...(r.payload.commit_hash ? { committedAt: r.payload.commit_hash } : {}),
      }));

      return { success: true, data: { results } };
    },
  };
}
