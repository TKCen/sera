import { Router } from 'express';
import type { MemoryManager } from '../memory/manager.js';
import { ScopedMemoryBlockStore } from '../memory/blocks/ScopedMemoryBlockStore.js';
import type { MemoryNamespace } from '../services/vector.service.js';
import { MemoryCompactionService } from '../memory/MemoryCompactionService.js';
import { EmbeddingService } from '../services/embedding.service.js';
import { extractLinks } from '../memory/blocks/scoped-types.js';
import type { KnowledgeBlock, KnowledgeBlockType } from '../memory/blocks/scoped-types.js';
import { CoreMemoryService } from '../memory/CoreMemoryService.js';
import { pool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';

const MEMORY_ROOT = process.env.MEMORY_PATH ?? '/memory';
const memLogger = new Logger('MemoryRoutes');

export function createMemoryRouter(memoryManager: MemoryManager): Router {
  const router = Router();
  const scopedStore = new ScopedMemoryBlockStore(MEMORY_ROOT);

  // ── Legacy Letta-style routes (Epic 5) ─────────────────────────────────────

  router.get('/blocks', async (_req, res) => {
    try {
      const blocks = await memoryManager.getAllBlocks();
      res.json(blocks);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  router.get('/blocks/:type', async (req, res) => {
    try {
      const { type } = req.params;
      const block = await memoryManager.getBlock(
        type as import('../memory/blocks/types.js').MemoryBlockType
      );
      res.json(block);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  router.post('/blocks/:type', async (req, res) => {
    try {
      const { type } = req.params;
      const entry = await memoryManager.addEntry(
        type as import('../memory/blocks/types.js').MemoryBlockType,
        req.body as import('../memory/blocks/types.js').CreateEntryOptions
      );
      res.status(201).json(entry);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  router.get('/entries/:id', async (req, res) => {
    try {
      const entry = await memoryManager.getEntry(req.params.id);
      if (!entry) return res.status(404).json({ error: 'Entry not found' });
      res.json(entry);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  router.delete('/entries/:id', async (req, res) => {
    try {
      const deleted = await memoryManager.deleteEntry(req.params.id);
      if (!deleted) return res.status(404).json({ error: 'Entry not found' });
      res.status(204).end();
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  router.get('/graph', async (_req, res) => {
    try {
      // Build graph from Epic 8 scoped blocks instead of legacy Letta-style graph
      const agentIds = await scopedStore.listAgentIds();
      const nodes: Array<{
        id: string;
        title: string;
        type: string;
        tags: string[];
        nodeKind: 'block' | 'agent';
        agentId?: string;
      }> = [];
      const edges: Array<{
        source: string;
        target: string;
        kind: string;
        relationship?: string;
      }> = [];

      for (const agentId of agentIds) {
        nodes.push({
          id: `agent:${agentId}`,
          title: agentId,
          type: 'agent',
          tags: [],
          nodeKind: 'agent',
        });

        const blocks = await scopedStore.list(agentId);
        for (const block of blocks) {
          nodes.push({
            id: block.id,
            title: block.title,
            type: block.type,
            tags: block.tags,
            nodeKind: 'block',
            agentId,
          });

          edges.push({
            source: `agent:${agentId}`,
            target: block.id,
            kind: 'owns',
          });

          const links = extractLinks(block.content);
          for (const link of links) {
            edges.push({
              source: block.id,
              target: link.target,
              kind: 'wikilink',
              relationship: link.relationship,
            });
          }
        }
      }

      res.json({ nodes, edges });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  // ── Cross-agent routes (#352) ──────────────────────────────────────────────
  // IMPORTANT: These must come BEFORE the /:agentId/* routes — Express 5
  // would otherwise match 'overview', 'recent', 'search' as an agentId param.

  /** GET /api/memory/overview — aggregate stats across all agents */
  router.get('/overview', async (_req, res) => {
    try {
      const agentIds = await scopedStore.listAgentIds();
      const agents: Array<{ id: string; blockCount: number }> = [];
      const tagCounts = new Map<string, number>();
      const typeCounts: Record<string, number> = {};
      let totalBlocks = 0;

      for (const agentId of agentIds) {
        const blocks = await scopedStore.list(agentId);
        if (blocks.length === 0) continue; // skip agents with no memory blocks
        agents.push({ id: agentId, blockCount: blocks.length });
        totalBlocks += blocks.length;
        for (const block of blocks) {
          typeCounts[block.type] = (typeCounts[block.type] ?? 0) + 1;
          for (const tag of block.tags) {
            tagCounts.set(tag, (tagCounts.get(tag) ?? 0) + 1);
          }
        }
      }

      const topTags = [...tagCounts.entries()]
        .map(([tag, count]) => ({ tag, count }))
        .sort((a, b) => b.count - a.count)
        .slice(0, 30);

      res.json({ totalBlocks, agents, topTags, typeBreakdown: typeCounts });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /** GET /api/memory/recent — recent blocks across all agents */
  router.get('/recent', async (req, res) => {
    try {
      const limit = Math.min(parseInt(req.query.limit as string, 10) || 20, 100);
      const blocks = await scopedStore.listAllBlocks({ limit });
      res.json(blocks);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /** GET /api/memory/search — semantic search across all agents */
  router.get('/search', async (req, res) => {
    try {
      const query = req.query.query as string;
      if (!query || query.length < 2) {
        res.status(400).json({ error: 'query must be at least 2 characters' });
        return;
      }
      const limit = Math.min(parseInt(req.query.limit as string, 10) || 10, 30);

      // Try semantic search first
      let embeddingService: EmbeddingService | undefined;
      try {
        embeddingService = EmbeddingService.getInstance();
      } catch {
        // Embedding service not configured — fall back to text search
      }

      if (embeddingService?.isAvailable()) {
        const vector = await embeddingService.embed(query);
        const agentIds = await scopedStore.listAgentIds();
        const namespaces: MemoryNamespace[] = agentIds.map(
          (id) => `personal:${id}` as MemoryNamespace
        );

        if (namespaces.length > 0) {
          const results = await memoryManager.vectorService.search(namespaces, vector, limit);
          const hydrated: Array<{ block: KnowledgeBlock | null; score: number }> = [];
          for (const result of results) {
            const agentId = result.payload?.agent_id as string | undefined;
            const blockId = result.id as string;
            if (agentId) {
              const block = await scopedStore.readByAgent(agentId, blockId);
              hydrated.push({ block, score: result.score ?? 0 });
            }
          }
          res.json(hydrated.filter((h) => h.block !== null));
          return;
        }
      }

      // Fallback: text search across all blocks
      memLogger.info('Semantic search unavailable, falling back to text search');
      const all = await scopedStore.listAllBlocks();
      const lowerQuery = query.toLowerCase();
      const matches = all
        .filter(
          (b) =>
            b.title.toLowerCase().includes(lowerQuery) ||
            b.content.toLowerCase().includes(lowerQuery) ||
            b.tags.some((t) => t.toLowerCase().includes(lowerQuery))
        )
        .slice(0, limit)
        .map((block) => ({ block, score: 1.0 }));
      res.json(matches);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /** GET /api/memory/explorer-graph — cross-agent graph with agent/circle meta-nodes */
  router.get('/explorer-graph', async (_req, res) => {
    try {
      const agentIds = await scopedStore.listAgentIds();
      const nodes: Array<{
        id: string;
        title: string;
        type: string;
        tags: string[];
        nodeKind: 'block' | 'agent' | 'circle';
        agentId?: string;
      }> = [];
      const edges: Array<{
        source: string;
        target: string;
        kind: string;
        relationship?: string;
      }> = [];

      for (const agentId of agentIds) {
        // Agent meta-node
        nodes.push({
          id: `agent:${agentId}`,
          title: agentId,
          type: 'agent',
          tags: [],
          nodeKind: 'agent',
        });

        const blocks = await scopedStore.list(agentId);
        for (const block of blocks) {
          // Block node
          nodes.push({
            id: block.id,
            title: block.title,
            type: block.type,
            tags: block.tags,
            nodeKind: 'block',
            agentId,
          });

          // Agent → block edge
          edges.push({
            source: `agent:${agentId}`,
            target: block.id,
            kind: 'owns',
          });

          // Wiki-link edges
          const links = extractLinks(block.content);
          for (const link of links) {
            edges.push({
              source: block.id,
              target: link.target,
              kind: 'wikilink',
              relationship: link.relationship,
            });
          }
        }
      }

      res.json({ nodes, edges });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  // ── Epic 8 scoped memory routes ────────────────────────────────────────────

  /** GET /api/memory/:agentId/stats */
  router.get('/:agentId/stats', async (req, res) => {
    try {
      const { agentId } = req.params;
      const blockCount = await scopedStore.countByAgent(agentId);

      const personalNs: MemoryNamespace = `personal:${agentId}`;
      const personalInfo = await memoryManager.vectorService.getCollectionInfo(personalNs);

      res.json({
        agentId,
        blockCount,
        vectorCount: personalInfo?.vectorCount ?? 0,
        namespaces: {
          personal: personalInfo ?? { vectorCount: 0 },
        },
      });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /** GET /api/memory/:agentId/blocks — list scoped blocks */
  router.get('/:agentId/blocks', async (req, res) => {
    try {
      const { agentId } = req.params;
      const query = req.query as Record<string, string | undefined>;
      const { type, tags, excludeTags, since, minImportance } = query;
      const blocks = await scopedStore.list(agentId, {
        ...(type ? { type: type as KnowledgeBlockType } : {}),
        ...(tags ? { tags: tags.split(',') } : {}),
        ...(excludeTags ? { excludeTags: excludeTags.split(',') } : {}),
        ...(since ? { since } : {}),
        ...(minImportance ? { minImportance: parseInt(minImportance, 10) } : {}),
      });
      res.json(blocks);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /** GET /api/memory/:agentId/blocks/:id */
  router.get('/:agentId/blocks/:id', async (req, res) => {
    try {
      const { agentId, id } = req.params;
      // Check active blocks first, then archive
      let block = await scopedStore.readByAgent(agentId, id);
      if (!block) {
        const archived = await scopedStore.listArchive(agentId);
        block = archived.find((b) => b.id === id) ?? null;
      }
      if (!block) return res.status(404).json({ error: 'Block not found' });
      res.json(block);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /** POST /api/memory/:agentId/compact — manual compaction trigger */
  router.post('/:agentId/compact', async (req, res) => {
    try {
      const { agentId } = req.params;
      const result = await MemoryCompactionService.getInstance().triggerCompaction(agentId);
      res.json(result);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  // ── Knowledge Links (Obsidian-style wiki-links in markdown content) ─────

  /** GET /api/memory/:agentId/links — extract all links from all blocks */
  router.get('/:agentId/links', async (req, res) => {
    try {
      const { agentId } = req.params;
      const entryId = req.query.entryId as string | undefined;
      const blocks = await scopedStore.list(agentId);
      const allLinks: Array<{
        sourceId: string;
        sourceTitle: string;
        target: string;
        relationship: string;
      }> = [];

      for (const block of blocks) {
        const links = extractLinks(block.content);
        for (const link of links) {
          if (!entryId || block.id === entryId || link.target === entryId) {
            allLinks.push({
              sourceId: block.id,
              sourceTitle: block.title,
              target: link.target,
              relationship: link.relationship,
            });
          }
        }
      }
      res.json(allLinks);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /** GET /api/memory/:agentId/graph — build a link graph from all blocks */
  router.get('/:agentId/graph', async (req, res) => {
    try {
      const { agentId } = req.params;
      const blocks = await scopedStore.list(agentId);
      const nodes: Array<{ id: string; title: string; type: string }> = [];
      const edges: Array<{ source: string; target: string; relationship: string }> = [];

      for (const block of blocks) {
        nodes.push({ id: block.id, title: block.title, type: block.type });
        const links = extractLinks(block.content);
        for (const link of links) {
          edges.push({ source: block.id, target: link.target, relationship: link.relationship });
        }
      }
      res.json({ nodes, edges });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /** GET /api/memory/:agentId/blocks/:id/backlinks — find blocks that link TO this one */
  router.get('/:agentId/blocks/:id/backlinks', async (req, res) => {
    try {
      const agentId = req.params.agentId as string;
      const targetId = req.params.id as string;
      const blocks = await scopedStore.list(agentId);
      const backlinks: Array<{
        sourceId: string;
        sourceTitle: string;
        sourceType: string;
        relationship: string;
      }> = [];

      for (const block of blocks) {
        if (block.id === targetId) continue;
        const links = extractLinks(block.content);
        for (const link of links) {
          if (link.target === targetId) {
            backlinks.push({
              sourceId: block.id,
              sourceTitle: block.title,
              sourceType: block.type,
              relationship: link.relationship,
            });
          }
        }
      }
      res.json(backlinks);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /** DELETE /api/memory/:agentId/blocks/:id — delete a block */
  router.delete('/:agentId/blocks/:id', async (req, res) => {
    try {
      const { agentId, id } = req.params;
      let deleted = await scopedStore.delete(agentId, id);
      if (!deleted) {
        deleted = await scopedStore.deleteArchive(agentId, id);
      }

      if (!deleted) {
        res.status(404).json({ error: 'Block not found' });
        return;
      }

      // Cleanup vector store
      const namespace: MemoryNamespace =
        agentId === 'global'
          ? 'global'
          : agentId.startsWith('circle:')
            ? (agentId as MemoryNamespace)
            : (`personal:${agentId}` as MemoryNamespace);

      await memoryManager.vectorService.delete(id, namespace);

      res.status(204).end();
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /** PUT /api/memory/:agentId/blocks/:id — update a block's content/metadata */
  router.put('/:agentId/blocks/:id', async (req, res) => {
    try {
      const agentId = req.params.agentId as string;
      const blockId = req.params.id as string;
      const { title, content, tags, importance } = req.body as {
        title?: string;
        content?: string;
        tags?: string[];
        importance?: number;
      };

      const updates: Parameters<typeof scopedStore.update>[2] = {};
      if (title !== undefined) updates.title = title;
      if (content !== undefined) updates.content = content;
      if (tags !== undefined) updates.tags = tags;
      if (importance !== undefined)
        updates.importance = importance as import('../memory/blocks/scoped-types.js').Importance;

      const updated = await scopedStore.update(agentId, blockId, updates);

      if (!updated) {
        res.status(404).json({ error: 'Block not found' });
        return;
      }

      res.json(updated);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /** POST /api/memory/:agentId/blocks/:id/promote — copy block to a wider scope */
  // ── Core Memory Block routes (Story 8.1) ──────────────────────────────────

  /** GET /api/memory/:agentId/core — fetch core memory blocks */
  router.get('/:agentId/core', async (req, res) => {
    try {
      const { agentId } = req.params;
      const coreMemoryService = CoreMemoryService.getInstance(pool);
      const blocks = await coreMemoryService.listBlocks(agentId);
      res.json(blocks);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /** PUT /api/memory/:agentId/core/:name — full update (operator use) */
  router.put('/:agentId/core/:name', async (req, res) => {
    try {
      const { agentId, name } = req.params;
      const { content, characterLimit, isReadOnly } = req.body as {
        content?: string;
        characterLimit?: number;
        isReadOnly?: boolean;
      };
      const coreMemoryService = CoreMemoryService.getInstance(pool);
      const updated = await coreMemoryService.updateBlock(agentId, name, {
        ...(content !== undefined ? { content } : {}),
        ...(characterLimit !== undefined ? { characterLimit } : {}),
        ...(isReadOnly !== undefined ? { isReadOnly } : {}),
      } as any);
      res.json(updated);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /** PATCH /api/memory/:agentId/core/:name — append/replace update (agent tool use) */
  router.patch('/:agentId/core/:name', async (req, res) => {
    try {
      const { agentId, name } = req.params;
      const { action, content, oldText, newText } = req.body as {
        action: 'append' | 'replace';
        content?: string;
        oldText?: string;
        newText?: string;
      };

      const coreMemoryService = CoreMemoryService.getInstance(pool);

      if (action === 'append') {
        if (typeof content !== 'string') {
          return res.status(400).json({ error: 'content is required for append action' });
        }
        const updated = await coreMemoryService.appendBlock(agentId, name, content);
        res.json(updated);
      } else if (action === 'replace') {
        if (typeof oldText !== 'string' || typeof newText !== 'string') {
          return res
            .status(400)
            .json({ error: 'oldText and newText are required for replace action' });
        }
        const updated = await coreMemoryService.replaceInBlock(agentId, name, oldText, newText);
        res.json(updated);
      } else {
        res.status(400).json({ error: 'invalid action' });
      }
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  router.post('/:agentId/blocks/:id/promote', async (req, res) => {
    try {
      const agentId = req.params.agentId as string;
      const blockId = req.params.id as string;
      const { targetScope, circleId } = req.body as {
        targetScope: 'circle' | 'global';
        circleId?: string;
      };

      if (!targetScope || !['circle', 'global'].includes(targetScope)) {
        res.status(400).json({ error: 'targetScope must be "circle" or "global"' });
        return;
      }

      const block = await scopedStore.readByAgent(agentId, blockId);
      if (!block) {
        res.status(404).json({ error: 'Block not found' });
        return;
      }

      // Write a copy to the target scope's namespace
      const targetAgentId = targetScope === 'circle' && circleId ? `circle:${circleId}` : 'global';
      const promoted = await scopedStore.write({
        agentId: targetAgentId,
        type: block.type,
        title: `[Promoted] ${block.title}`,
        content: block.content,
        tags: [...block.tags, `promoted-from:${agentId}`],
        importance: block.importance,
      });

      res.status(201).json(promoted);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  return router;
}
