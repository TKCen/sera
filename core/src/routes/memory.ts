import { Router } from 'express';
import type { MemoryManager } from '../memory/manager.js';
import { ScopedMemoryBlockStore } from '../memory/blocks/ScopedMemoryBlockStore.js';
import { VectorService } from '../services/vector.service.js';
import type { MemoryNamespace } from '../services/vector.service.js';
import { MemoryCompactionService } from '../memory/MemoryCompactionService.js';
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
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  router.get('/blocks/:type', async (req, res) => {
    try {
      const block = await memoryManager.getBlock(req.params.type as any);
      res.json(block);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  router.post('/blocks/:type', async (req, res) => {
    try {
      const entry = await memoryManager.addEntry(req.params.type as any, req.body);
      res.status(201).json(entry);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  router.get('/entries/:id', async (req, res) => {
    try {
      const entry = await memoryManager.getEntry(req.params.id);
      if (!entry) return res.status(404).json({ error: 'Entry not found' });
      res.json(entry);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  router.get('/graph', async (_req, res) => {
    try {
      const graph = await memoryManager.getGraph();
      res.json(graph);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
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
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** GET /api/memory/:agentId/blocks — list scoped blocks */
  router.get('/:agentId/blocks', async (req, res) => {
    try {
      const { agentId } = req.params;
      const { type, tags, since } = req.query as Record<string, string | undefined>;
      const blocks = await scopedStore.list(agentId, {
        ...(type ? { type: type as KnowledgeBlockType } : {}),
        ...(tags ? { tags: tags.split(',') } : {}),
        ...(since ? { since } : {}),
      });
      res.json(blocks);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
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
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** POST /api/memory/:agentId/compact — manual compaction trigger */
  router.post('/:agentId/compact', async (req, res) => {
    try {
      const { agentId } = req.params;
      const result = await MemoryCompactionService.getInstance().triggerCompaction(agentId);
      res.json(result);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  return router;
}
