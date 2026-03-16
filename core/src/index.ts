import express from 'express';
import { v4 as uuidv4 } from 'uuid';
import cors from 'cors';
import { IngestionService } from './services/ingestion.service.js';
import { EmbeddingService } from './services/embedding.service.js';
import { VectorService } from './services/vector.service.js';
import { initDb } from './lib/database.js';
import { config } from './lib/config.js';
import { PROVIDER_CATALOG } from './lib/providers.js';
import { Orchestrator } from './agents/Orchestrator.js';
import { PrimaryAgent } from './agents/PrimaryAgent.js';
import { WorkerAgent } from './agents/WorkerAgent.js';
import { OpenAIProvider } from './lib/llm/OpenAIProvider.js';
import { MCPRegistry } from './mcp/registry.js';
import { MemoryManager } from './memory/manager.js';
import lspRouter, { lspManager } from './routes/lsp.js';

const app = express();

const llmProvider = new OpenAIProvider();
const orchestrator = new Orchestrator();
const mcpRegistry = MCPRegistry.getInstance();
const memoryManager = new MemoryManager();

// Register agents
orchestrator.registerAgent(new PrimaryAgent(llmProvider));
orchestrator.registerAgent(new WorkerAgent('Sera-Researcher', 'researcher', llmProvider));

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

// ─── Chat API ─────────────────────────────────────────────────────────────────
const conversations = new Map<string, { role: string; content: string }[]>();

app.post('/api/chat', async (req, res) => {
  try {
    const { message, conversationId: incomingId } = req.body;
    if (!message) {
      return res.status(400).json({ error: 'message is required' });
    }

    const conversationId = incomingId || uuidv4();

    // Get or create conversation history
    if (!conversations.has(conversationId)) {
      conversations.set(conversationId, []);
    }
    const history = conversations.get(conversationId)!;
    history.push({ role: 'user', content: message });

    // Process through the primary agent
    const primaryAgent = orchestrator.getAgent('primary');
    if (!primaryAgent) {
      return res.status(500).json({ error: 'Primary agent not registered' });
    }

    const response = await primaryAgent.process(message);
    const reply = response.finalAnswer || response.thought || 'No response generated.';

    history.push({ role: 'assistant', content: reply });

    res.json({
      conversationId,
      reply,
      thought: response.thought,
    });
  } catch (error: any) {
    console.error('Chat error:', error);
    res.status(500).json({ error: error.message });
  }
});

// ─── Config API ───────────────────────────────────────────────────────────────
app.get('/api/config/llm', (req, res) => {
  res.json(config.llm);
});

app.post('/api/config/llm', (req, res) => {
  try {
    const newConfig = req.body;
    config.saveLlmConfig(newConfig);
    
    // Re-initialize LLM provider and agents
    const newLlmProvider = new OpenAIProvider();
    orchestrator.updateLlmProvider(newLlmProvider);
    
    res.json({ success: true });
  } catch (error: any) {
    res.status(500).json({ error: error.message });
  }
});

app.post('/api/config/llm/test', async (req, res) => {
  try {
    const testProvider = new OpenAIProvider();
    const response = await testProvider.chat([
      { role: 'user', content: 'Respond with exactly: CONNECTION_OK' }
    ]);
    res.json({
      success: true,
      model: config.llm.model,
      response: response.content.substring(0, 100),
    });
  } catch (error: any) {
    res.json({
      success: false,
      error: error.message,
    });
  }
});

// ─── Provider Management API ──────────────────────────────────────────────────

app.get('/api/providers', (req, res) => {
  const savedConfig = config.providers;
  const catalog = PROVIDER_CATALOG.map(provider => ({
    ...provider,
    configured: !!savedConfig.providers[provider.id]?.baseUrl,
    isActive: savedConfig.activeProvider === provider.id,
    savedConfig: savedConfig.providers[provider.id] || null,
  }));
  res.json({
    activeProvider: savedConfig.activeProvider,
    providers: catalog,
  });
});

app.put('/api/providers/:id', (req, res) => {
  try {
    const { id } = req.params;
    const { baseUrl, apiKey, model } = req.body;

    // Verify provider exists in catalog
    const provider = PROVIDER_CATALOG.find(p => p.id === id);
    if (!provider) {
      return res.status(404).json({ error: `Provider '${id}' not found` });
    }

    config.saveProviderConfig(id, { baseUrl, apiKey, model });

    // If this is the active provider, re-init the LLM provider
    if (config.providers.activeProvider === id) {
      const newLlmProvider = new OpenAIProvider();
      orchestrator.updateLlmProvider(newLlmProvider);
    }

    res.json({ success: true });
  } catch (error: any) {
    res.status(500).json({ error: error.message });
  }
});

app.post('/api/providers/:id/test', async (req, res) => {
  try {
    const { id } = req.params;
    const providerConfig = config.getProviderConfig(id);
    const catalogEntry = PROVIDER_CATALOG.find(p => p.id === id);

    if (!providerConfig?.baseUrl) {
      return res.json({ success: false, error: 'Provider not configured. Save settings first.' });
    }

    // Create a temporary provider with this config
    const testProvider = new OpenAIProvider({
      baseUrl: providerConfig.baseUrl,
      apiKey: providerConfig.apiKey || 'not-needed',
      model: providerConfig.model || catalogEntry?.models[0]?.id || 'test',
    });

    const response = await testProvider.chat([
      { role: 'user', content: 'Respond with exactly: CONNECTION_OK' }
    ]);

    res.json({
      success: true,
      provider: catalogEntry?.name || id,
      response: response.content.substring(0, 100),
    });
  } catch (error: any) {
    res.json({
      success: false,
      error: error.message,
    });
  }
});

app.post('/api/providers/active', (req, res) => {
  try {
    const { providerId } = req.body;
    if (!providerId) {
      return res.status(400).json({ error: 'providerId is required' });
    }

    const provider = PROVIDER_CATALOG.find(p => p.id === providerId);
    if (!provider) {
      return res.status(404).json({ error: `Provider '${providerId}' not found` });
    }

    config.setActiveProvider(providerId);

    // Re-init the LLM provider with the new active config
    const newLlmProvider = new OpenAIProvider();
    orchestrator.updateLlmProvider(newLlmProvider);

    res.json({ success: true, activeProvider: providerId });
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
