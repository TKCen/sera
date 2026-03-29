import { Router } from 'express';
import { asyncHandler } from '../middleware/asyncHandler.js';
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

  router.get(
    '/blocks',
    asyncHandler(async (_req, res) => {
      const blocks = await memoryManager.getAllBlocks();
      res.json(blocks);
    })
  );

  router.get(
    '/blocks/:type',
    asyncHandler(async (req, res) => {
      const { type } = req.params;
      const block = await memoryManager.getBlock(
        type as import('../memory/blocks/types.js').MemoryBlockType
      );
      res.json(block);
    })
  );

  router.post(
    '/blocks/:type',
    asyncHandler(async (req, res) => {
      const { type } = req.params;
      const entry = await memoryManager.addEntry(
        type as import('../memory/blocks/types.js').MemoryBlockType,
        req.body as import('../memory/blocks/types.js').CreateEntryOptions
      );
      res.status(201).json(entry);
    })
  );

  router.get(
    '/entries/:id',
    asyncHandler(async (req, res) => {
      const entry = await memoryManager.getEntry(req.params.id as string);
      if (!entry) {
        res.status(404).json({ error: 'Entry not found' });
        return;
      }
      res.json(entry);
    })
  );

  router.get(
    '/graph',
    asyncHandler(async (_req, res) => {
      const graph = await memoryManager.getGraph();
      res.json(graph);
    })
  );

  // ── Epic 8 scoped memory routes ────────────────────────────────────────────

  /** GET /api/memory/:agentId/stats */
  router.get(
    '/:agentId/stats',
    asyncHandler(async (req, res) => {
      const agentId = req.params.agentId as string;
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
    })
  );

  /** GET /api/memory/:agentId/blocks — list scoped blocks */
  router.get(
    '/:agentId/blocks',
    asyncHandler(async (req, res) => {
      const agentId = req.params.agentId as string;
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
    })
  );

  /** GET /api/memory/:agentId/blocks/:id */
  router.get(
    '/:agentId/blocks/:id',
    asyncHandler(async (req, res) => {
      const agentId = req.params.agentId as string;
      const id = req.params.id as string;
      // Check active blocks first, then archive
      let block = await scopedStore.readByAgent(agentId, id);
      if (!block) {
        const archived = await scopedStore.listArchive(agentId);
        block = archived.find((b) => b.id === id) ?? null;
      }
      if (!block) {
        res.status(404).json({ error: 'Block not found' });
        return;
      }
      res.json(block);
    })
  );

  /** POST /api/memory/:agentId/compact — manual compaction trigger */
  router.post(
    '/:agentId/compact',
    asyncHandler(async (req, res) => {
      const agentId = req.params.agentId as string;
      const result = await MemoryCompactionService.getInstance().triggerCompaction(agentId);
      res.json(result);
    })
  );

  // ── Knowledge Links (Obsidian-style wiki-links in markdown content) ─────

  /** GET /api/memory/:agentId/links — extract all links from all blocks */
  router.get(
    '/:agentId/links',
    asyncHandler(async (req, res) => {
      const agentId = req.params.agentId as string;
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
    })
  );

  /** GET /api/memory/:agentId/graph — build a link graph from all blocks */
  router.get(
    '/:agentId/graph',
    asyncHandler(async (req, res) => {
      const agentId = req.params.agentId as string;
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
    })
  );

  return router;
}
