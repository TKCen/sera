import express from 'express';
import cors from 'cors';
import { IngestionService } from './services/ingestion.service.js';
import { EmbeddingService } from './services/embedding.service.js';
import { VectorService } from './services/vector.service.js';
import { initDb } from './lib/database.js';
import { Orchestrator } from './agents/Orchestrator.js';
import { PrimaryAgent } from './agents/PrimaryAgent.js';
import { WorkerAgent } from './agents/WorkerAgent.js';
import { MCPRegistry } from './mcp/registry.js';
import { MemoryManager } from './memory/manager.js';
import lspRouter, { lspManager } from './routes/lsp.js';

const app = express();

const orchestrator = new Orchestrator();
const mcpRegistry = MCPRegistry.getInstance();
const memoryManager = new MemoryManager();

// Register agents
orchestrator.registerAgent(new PrimaryAgent());
orchestrator.registerAgent(new WorkerAgent('Sera-Researcher', 'researcher'));

app.use(cors());
app.use(express.json());
app.use('/api/lsp', lspRouter);

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

app.post('/api/ingest', async (req, res) => {
  try {
    const ingestionService = new IngestionService();
    // Non-blocking ingestion
    ingestionService.ingestCodebase().catch(err => console.error('Ingestion error:', err));
    res.json({ message: 'Ingestion started' });
  } catch (error: any) {
    res.status(500).json({ error: error.message });
  }
});

app.post('/api/query', async (req, res) => {
  try {
    const { query, limit } = req.body;
    if (!query) {
      return res.status(400).json({ error: 'Query is required' });
    }

    const embeddingService = EmbeddingService.getInstance();
    const vectorService = new VectorService();

    const vector = await embeddingService.generateEmbedding(query);
    const results = await vectorService.search(vector, limit || 5);

    res.json({ results });
  } catch (error: any) {
    res.status(500).json({ error: error.message });
  }
});

app.post('/api/execute', async (req, res) => {
  const { prompt } = req.body;
  try {
    const result = await orchestrator.executeTask(prompt);
    res.json({ result });
  } catch (error: any) {
    res.status(500).json({ error: error.message });
  }
});

app.get('/api/tools', async (req, res) => {
  try {
    const tools = await mcpRegistry.getAllTools();
    res.json({ tools });
  } catch (error: any) {
    res.status(500).json({ error: error.message });
  }
});

export { app };

const shutdown = async () => {
  console.log('Shutting down SERA Core...');
  await lspManager.stopAll();
};

process.on('SIGTERM', shutdown);
process.on('SIGINT', shutdown);

if (process.env.NODE_ENV !== 'test') {
  const port = process.env.PORT || 3001;
  initDb().then(() => {
    app.listen(port, () => {
      console.log(`SERA Core orchestrator listening at http://localhost:${port}`);
    });
  }).catch(err => {
    console.error('Failed to start SERA Core:', err);
    process.exit(1);
  });
}
