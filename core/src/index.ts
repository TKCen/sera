import express from 'express';
import cors from 'cors';
import { MemoryManager } from './memory/manager.js';

const app = express();
const port = process.env.PORT || 3001;
const memoryManager = new MemoryManager();

app.use(cors());
app.use(express.json());

app.get('/api/health', (req, res) => {
  res.json({
    status: 'ok',
    service: 'sera-core',
    timestamp: new Date().toISOString()
  });
});

// Memory API endpoints
app.post('/api/memory/archive', async (req, res) => {
  try {
    const { title, content, tags } = req.body;
    if (!title || !content) {
      return res.status(400).json({ error: 'Title and content are required' });
    }
    const path = await memoryManager.archive(title, content, tags);
    res.json({ success: true, path });
  } catch (err: any) {
    res.status(500).json({ error: err.message });
  }
});

app.get('/api/memory/search', async (req, res) => {
  try {
    const { query, tags, limit } = req.query;
    const results = await memoryManager.searchArchival({
      query: query as string,
      tags: tags ? (Array.isArray(tags) ? tags as string[] : [tags as string]) : undefined,
      limit: limit ? parseInt(limit as string) : undefined
    });
    res.json(results);
  } catch (err: any) {
    res.status(500).json({ error: err.message });
  }
});

app.get('/api/memory/working', (req, res) => {
  res.json(memoryManager.getWorkingMemory());
});

app.post('/api/memory/working', (req, res) => {
  const { info } = req.body;
  if (!info) {
    return res.status(400).json({ error: 'Info is required' });
  }
  memoryManager.addToWorkingMemory(info);
  res.json({ success: true });
});

app.listen(port, () => {
  console.log(`SERA Core orchestrator listening at http://localhost:${port}`);
});
