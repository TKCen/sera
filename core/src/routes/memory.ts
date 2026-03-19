import { Router } from 'express';
import type { Request, Response } from 'express';
import type { MemoryManager } from '../memory/manager.js';

export function createMemoryRouter(memoryManager: MemoryManager) {
  const router = Router();

  /** GET /api/memory/blocks — list all blocks */
  router.get('/blocks', async (req, res) => {
    try {
      const blocks = await memoryManager.store.listBlocks();
      res.json(blocks);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** GET /api/memory/blocks/:type — get a specific block */
  router.get('/blocks/:type', async (req, res) => {
    try {
      const block = await memoryManager.getBlock(req.params.type as any);
      res.json(block);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** POST /api/memory/blocks/:type — create an entry */
  router.post('/blocks/:type', async (req, res) => {
    try {
      const entry = await memoryManager.addEntry(req.params.type as any, req.body);
      res.status(201).json(entry);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** GET /api/memory/entries/:id — get an entry by ID */
  router.get('/entries/:id', async (req, res) => {
    try {
      const entry = await memoryManager.store.getEntry(req.params.id);
      if (!entry) return res.status(404).json({ error: 'Entry not found' });
      res.json(entry);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** GET /api/memory/graph — get full memory graph */
  router.get('/graph', async (req, res) => {
    try {
      const graph = await memoryManager.getGraph();
      res.json(graph);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  return router;
}
