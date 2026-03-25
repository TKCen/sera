import { Router } from 'express';
import type { MemoryManager } from '../memory/manager.js';
import { ScopedMemoryBlockStore } from '../memory/blocks/ScopedMemoryBlockStore.js';
import { VectorService } from '../services/vector.service.js';
import type { MemoryNamespace } from '../services/vector.service.js';
import { MemoryCompactionService } from '../memory/MemoryCompactionService.js';
import { extractLinks } from '../memory/blocks/scoped-types.js';
import type { KnowledgeBlockType } from '../memory/blocks/scoped-types.js';

const MEMORY_ROOT = process.env.MEMORY_PATH ?? '/memory';

export function createMemoryRouter(memoryManager: MemoryManager) {
  const router = Router();
  const scopedStore = new ScopedMemoryBlockStore(MEMORY_ROOT);
  const vectorService = new VectorService('_mem_route_unused');

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

  router.get('/graph', async (_req, res) => {
    try {
      const graph = await memoryManager.getGraph();
      res.json(graph);
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
      const personalInfo = await vectorService.getCollectionInfo(personalNs);

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

  return router;
}
