import { Router } from 'express';
import type { MemoryManager } from '../memory/manager.js';
import type { MemoryBlockType } from '../memory/blocks/types.js';

export function createMemoryRouter(memoryManager: MemoryManager) {
  const router = Router();

  /** GET /api/memory/blocks — Retrieves all specialized memory blocks. */
  router.get('/blocks', async (req, res) => {
    try {
      const blocks = await memoryManager.getAllBlocks();
      res.json(blocks);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** GET /api/memory/blocks/:type — Retrieves a specific memory block type. */
  router.get('/blocks/:type', async (req, res) => {
    try {
      const type = req.params.type as MemoryBlockType;
      const block = await memoryManager.getBlock(type);
      res.json(block);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** POST /api/memory/blocks/:type — Adds an entry to a specific memory block. */
  router.post('/blocks/:type', async (req, res) => {
    try {
      const type = req.params.type as MemoryBlockType;
      const entry = await memoryManager.addEntry(type, req.body);
      res.status(201).json(entry);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** GET /api/memory/entries/:id — Retrieves a specific memory entry by ID. */
  router.get('/entries/:id', async (req, res) => {
    try {
      const entry = await memoryManager.getEntry(req.params.id);
      if (!entry) {
        return res.status(404).json({ error: 'Entry not found' });
      }
      res.json(entry);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** PUT /api/memory/entries/:id — Updates the content of a memory entry. */
  router.put('/entries/:id', async (req, res) => {
    try {
      const entry = await memoryManager.updateEntry(req.params.id, req.body.content);
      if (!entry) {
        return res.status(404).json({ error: 'Entry not found' });
      }
      res.json(entry);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** DELETE /api/memory/entries/:id — Deletes a memory entry. */
  router.delete('/entries/:id', async (req, res) => {
    try {
      const success = await memoryManager.deleteEntry(req.params.id);
      if (!success) {
        return res.status(404).json({ error: 'Entry not found' });
      }
      res.json({ success: true });
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** POST /api/memory/entries/:id/refs — Links one memory entry to another. */
  router.post('/entries/:id/refs', async (req, res) => {
    try {
      const success = await memoryManager.addRef(req.params.id, req.body.targetId);
      if (!success) {
        return res.status(404).json({ error: 'Entry not found' });
      }
      res.json({ success: true });
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** DELETE /api/memory/entries/:id/refs/:targetId — Removes a link between memory entries. */
  router.delete('/entries/:id/refs/:targetId', async (req, res) => {
    try {
      const success = await memoryManager.removeRef(req.params.id, req.params.targetId);
      if (!success) {
        return res.status(404).json({ error: 'Entry or reference not found' });
      }
      res.json({ success: true });
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** GET /api/memory/graph — Returns the memory structure as a node-link graph. */
  router.get('/graph', async (req, res) => {
    try {
      const graph = await memoryManager.getGraph();
      res.json(graph);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /** GET /api/memory/search — Performs a semantic search across memory entries. */
  router.get('/search', async (req, res) => {
    try {
      const query = req.query.query as string;
      const limit = Number(req.query.limit) || 5;
      const results = await memoryManager.search(query, limit);
      res.json(results);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  return router;
}
