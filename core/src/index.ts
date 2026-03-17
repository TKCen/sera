import path from 'path';
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
import { OpenAIProvider } from './lib/llm/OpenAIProvider.js';
import { MCPRegistry } from './mcp/registry.js';
import { MemoryManager } from './memory/manager.js';
import type { MemoryBlockType } from './memory/types.js';
import { MEMORY_BLOCK_TYPES } from './memory/types.js';
import { CircleRegistry } from './circles/CircleRegistry.js';
import { AgentManifestLoader } from './agents/manifest/AgentManifestLoader.js';
import lspRouter, { lspManager } from './routes/lsp.js';
import { SandboxManager } from './sandbox/SandboxManager.js';
import { Logger } from './lib/logger.js';

const logger = new Logger('Server');
import { createSandboxRouter } from './routes/sandbox.js';
import { IntercomService } from './intercom/IntercomService.js';
import { createIntercomRouter } from './routes/intercom.js';
import { SkillRegistry } from './skills/SkillRegistry.js';
import { registerBuiltinSkills } from './skills/builtins/index.js';
import { PartySessionManager } from './circles/PartyMode.js';
import { createAgentRouter } from './routes/agents.js';
import { createCircleRouter } from './routes/circles.js';
import { createSkillsRouter } from './routes/skills.js';
const app = express();

// ── Workspace Root ───────────────────────────────────────────────────────────
// In Docker the workspace is mounted at /app/workspace but the compiled JS
// lives at /app/dist. WORKSPACE_DIR overrides the default resolution.
const workspaceRoot = process.env.WORKSPACE_DIR
  ?? path.resolve(import.meta.dirname, '..', '..');

// ── Agent System ──────────────────────────────────────────────────────────────
const orchestrator = new Orchestrator();
const agentsDir = path.join(workspaceRoot, 'agents');
orchestrator.loadAgentsFromManifests(agentsDir);

// ── Circle System ─────────────────────────────────────────────────────────────
const circleRegistry = new CircleRegistry();
const circlesDir = path.join(workspaceRoot, 'circles');
const agentManifests = AgentManifestLoader.loadAllManifests(agentsDir);
await circleRegistry.loadFromDirectory(circlesDir, agentManifests);

// ── Sandbox Manager ──────────────────────────────────────────────────────────
const sandboxManager = new SandboxManager();
const sandboxRouter = createSandboxRouter(sandboxManager, (agentName: string) => {
  return agentManifests.find(m => m.metadata.name === agentName);
});

// ── Intercom Service ─────────────────────────────────────────────────────────
const intercomService = new IntercomService();
const intercomRouter = createIntercomRouter(intercomService, (agentName: string) => {
  return agentManifests.find(m => m.metadata.name === agentName);
});

orchestrator.setIntercom(intercomService);

const mcpRegistry = MCPRegistry.getInstance();
const memoryManager = new MemoryManager();

// ── Skills System ────────────────────────────────────────────────────────────
const skillRegistry = new SkillRegistry();
registerBuiltinSkills(skillRegistry, memoryManager);
// MCP tools are bridged asynchronously — they'll be available after server start
mcpRegistry.getAllTools().then(async () => {
  const count = await skillRegistry.bridgeMCPTools(mcpRegistry);
  if (count > 0) {
    const logger = new Logger('SkillRegistry');
    logger.info(`Bridged ${count} MCP tool(s) as skills`);
  }
}).catch(() => { /* MCP servers may not be connected yet */ });

// ── Route Modules ────────────────────────────────────────────────────────────
const agentRouter = createAgentRouter(orchestrator, agentsDir);
const circleRouter = createCircleRouter(
  circleRegistry,
  circlesDir,
  () => AgentManifestLoader.loadAllManifests(agentsDir),
  orchestrator,
);
const skillsRouter = createSkillsRouter(skillRegistry, orchestrator);

// ── Party Mode ───────────────────────────────────────────────────────────────
const partySessionManager = new PartySessionManager();

// ── Agent File Watcher ───────────────────────────────────────────────────────
orchestrator.watchAgentsDirectory(agentsDir);

app.use(cors());
app.use(express.json());
app.use('/api/lsp', lspRouter);
app.use('/api/sandbox', sandboxRouter);
app.use('/api/intercom', intercomRouter);
app.use('/api/agents', agentRouter);
app.use('/api/circles', circleRouter);
app.use('/api/skills', skillsRouter);

/**
 * Health check endpoint.
 * @param req Express request
 * @param res Express response
 * @returns {void}
 */
app.get('/api/health', (req, res) => {
  res.json({
    status: 'ok',
    service: 'sera-core',
    timestamp: new Date().toISOString()
  });
});

// ─── Agents, Circles, and Skills routes are now handled by route modules ────

// ─── Party Mode API ──────────────────────────────────────────────────────────

/**
 * Creates a party session for a circle.
 * @param req Express request containing circleId in params
 * @param res Express response
 * @returns {void}
 */
app.post('/api/circles/:circleId/party', (req, res) => {
  try {
    const circle = circleRegistry.getCircle(req.params.circleId);
    if (!circle) {
      return res.status(404).json({ error: `Circle "${req.params.circleId}" not found` });
    }

    // Gather agents from the orchestrator that belong to this circle
    const agents = new Map<string, any>();
    for (const agentName of circle.agents) {
      const agent = orchestrator.getAgent(agentName);
      if (agent) agents.set(agentName, agent);
    }

    const orchestratorAgentName = circle.partyMode?.orchestrator;
    const orchestratorAgentInstance = orchestratorAgentName
      ? orchestrator.getAgent(orchestratorAgentName)
      : undefined;

    const session = partySessionManager.createSession(
      circle,
      agents,
      orchestratorAgentInstance,
    );

    res.status(201).json(session.getInfo());
  } catch (err: any) {
    res.status(400).json({ error: err.message });
  }
});

/**
 * Sends a message to a party session.
 * @param req Express request containing message in body
 * @param res Express response
 * @returns {Promise<void>}
 */
app.post('/api/circles/:circleId/party/:sessionId', async (req, res) => {
  try {
    const session = partySessionManager.getSession(req.params.sessionId);
    if (!session) {
      return res.status(404).json({ error: 'Party session not found' });
    }

    const { message } = req.body;
    if (!message) {
      return res.status(400).json({ error: 'message is required' });
    }

    const responses = await session.sendMessage(message);
    res.json({
      sessionId: req.params.sessionId,
      responses,
      active: session.isActive(),
    });
  } catch (err: any) {
    res.status(500).json({ error: err.message });
  }
});

/**
 * Ends a party session.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
app.delete('/api/circles/:circleId/party/:sessionId', async (req, res) => {
  const ended = partySessionManager.endSession(req.params.sessionId);
  if (!ended) {
    return res.status(404).json({ error: 'Party session not found' });
  }
  res.json({ success: true });
});

/**
 * Lists party sessions for a circle.
 * @param req Express request
 * @param res Express response
 * @returns {void}
 */
app.get('/api/circles/:circleId/party', (req, res) => {
  res.json(partySessionManager.listSessions(req.params.circleId));
});

// ─── Memory Blocks API ────────────────────────────────────────────────────────

/**
 * Gets all memory blocks.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
app.get('/api/memory/blocks', async (req, res) => {
  try {
    const blocks = await memoryManager.getAllBlocks();
    res.json(blocks);
  } catch (err: any) {
    res.status(500).json({ error: err.message });
  }
});

/**
 * Gets a specific memory block by type.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
app.get('/api/memory/blocks/:type', async (req, res) => {
  const type = req.params.type as MemoryBlockType;
  if (!MEMORY_BLOCK_TYPES.includes(type)) {
    return res.status(400).json({ error: `Invalid block type "${type}". Must be one of: ${MEMORY_BLOCK_TYPES.join(', ')}` });
  }
  try {
    const block = await memoryManager.getBlock(type);
    res.json(block);
  } catch (err: any) {
    res.status(500).json({ error: err.message });
  }
});

/**
 * Adds an entry to a specific memory block.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
app.post('/api/memory/blocks/:type', async (req, res) => {
  const type = req.params.type as MemoryBlockType;
  if (!MEMORY_BLOCK_TYPES.includes(type)) {
    return res.status(400).json({ error: `Invalid block type "${type}"` });
  }
  const { title, content, refs, tags, source } = req.body;
  if (!title || !content) {
    return res.status(400).json({ error: 'title and content are required' });
  }
  try {
    const entry = await memoryManager.addEntry(type, { title, content, refs, tags, source });
    res.status(201).json(entry);
  } catch (err: any) {
    res.status(500).json({ error: err.message });
  }
});

// ─── Memory Entries API ───────────────────────────────────────────────────────

/**
 * Gets a specific memory entry.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
app.get('/api/memory/entries/:id', async (req, res) => {
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

/**
 * Updates a memory entry.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
app.put('/api/memory/entries/:id', async (req, res) => {
  const { content } = req.body;
  if (!content) {
    return res.status(400).json({ error: 'content is required' });
  }
  try {
    const entry = await memoryManager.updateEntry(req.params.id, content);
    if (!entry) {
      return res.status(404).json({ error: 'Entry not found' });
    }
    res.json(entry);
  } catch (err: any) {
    res.status(500).json({ error: err.message });
  }
});

/**
 * Deletes a memory entry.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
app.delete('/api/memory/entries/:id', async (req, res) => {
  try {
    const deleted = await memoryManager.deleteEntry(req.params.id);
    if (!deleted) {
      return res.status(404).json({ error: 'Entry not found' });
    }
    res.json({ success: true });
  } catch (err: any) {
    res.status(500).json({ error: err.message });
  }
});

// ─── Memory Refs API ──────────────────────────────────────────────────────────

/**
 * Adds a reference link between memory entries.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
app.post('/api/memory/entries/:id/refs', async (req, res) => {
  const { targetId } = req.body;
  if (!targetId) {
    return res.status(400).json({ error: 'targetId is required' });
  }
  try {
    const ok = await memoryManager.addRef(req.params.id, targetId);
    if (!ok) {
      return res.status(404).json({ error: 'Source entry not found' });
    }
    res.json({ success: true });
  } catch (err: any) {
    res.status(500).json({ error: err.message });
  }
});

/**
 * Removes a reference link.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
app.delete('/api/memory/entries/:id/refs/:targetId', async (req, res) => {
  try {
    const ok = await memoryManager.removeRef(req.params.id, req.params.targetId);
    if (!ok) {
      return res.status(404).json({ error: 'Ref not found' });
    }
    res.json({ success: true });
  } catch (err: any) {
    res.status(500).json({ error: err.message });
  }
});

// ─── Memory Graph API ─────────────────────────────────────────────────────────

/**
 * Gets the memory graph.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
app.get('/api/memory/graph', async (req, res) => {
  try {
    const graph = await memoryManager.getGraph();
    res.json(graph);
  } catch (err: any) {
    res.status(500).json({ error: err.message });
  }
});

/**
 * Searches memory entries.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
app.get('/api/memory/search', async (req, res) => {
  try {
    const { query, limit } = req.query;
    if (!query) {
      return res.status(400).json({ error: 'query parameter is required' });
    }
    const results = await memoryManager.search(
      query as string,
      limit ? parseInt(limit as string) : undefined,
    );
    res.json(results);
  } catch (err: any) {
    res.status(500).json({ error: err.message });
  }
});

/**
 * Triggers codebase ingestion.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
app.post('/api/ingest', async (req, res) => {
  try {
    const ingestionService = new IngestionService();
    // Non-blocking ingestion
    ingestionService.ingestCodebase().catch(err => logger.error('Ingestion error:', err));
    res.json({ message: 'Ingestion started' });
  } catch (error: any) {
    res.status(500).json({ error: error.message });
  }
});

/**
 * Queries the vector database.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
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

/**
 * Executes a task using the orchestrator.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
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
import type { ChatMessage } from './agents/types.js';
const conversations = new Map<string, ChatMessage[]>();

/**
 * Sends a chat message.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
app.post('/api/chat', async (req, res) => {
  try {
    const { message, conversationId: incomingId, agentName } = req.body;
    if (!message) {
      return res.status(400).json({ error: 'message is required' });
    }

    const conversationId = incomingId || uuidv4();

    // Get or create conversation history
    if (!conversations.has(conversationId)) {
      conversations.set(conversationId, []);
    }
    const history = conversations.get(conversationId)!;

    // Process through the specified agent or the primary agent
    let agent;
    if (agentName) {
      agent = orchestrator.getAgent(agentName);
      if (!agent) {
        return res.status(404).json({ error: `Agent "${agentName}" not found.` });
      }
    } else {
      agent = orchestrator.getPrimaryAgent();
      if (!agent) {
        return res.status(500).json({ error: 'No primary agent configured. Check your AGENT.yaml manifests.' });
      }
    }

    try {
      const response = await agent.process(message, history);
      const reply = response.finalAnswer || response.thought || 'No response generated.';

      history.push({ role: 'user', content: message });
      history.push({ role: 'assistant', content: reply });

      res.json({
        conversationId,
        reply,
        thought: response.thought,
      });
    } catch (agentError: any) {
      logger.error(`[${agent.name}] Error during processing:`, agentError);
      if (agentError.name === 'AbortError' || agentError.message.includes('timeout')) {
         return res.status(504).json({ error: `Agent "${agent.name}" timed out while processing.` });
      }
      return res.status(500).json({ error: `LLM error from "${agent.name}": ${agentError.message}` });
    }
  } catch (error: any) {
    logger.error('Chat API error:', error);
    res.status(500).json({ error: error.message });
  }
});

/**
 * Streams a chat response via Centrifugo.
 * Returns immediately with { conversationId, messageId }.
 * Tokens are published to `internal:stream:{messageId}`.
 */
app.post('/api/chat/stream', async (req, res) => {
  try {
    const { message, conversationId: incomingId, agentName } = req.body;
    if (!message) {
      return res.status(400).json({ error: 'message is required' });
    }

    const conversationId = incomingId || uuidv4();
    const messageId = uuidv4();

    if (!conversations.has(conversationId)) {
      conversations.set(conversationId, []);
    }
    const history = conversations.get(conversationId)!;

    let agent;
    if (agentName) {
      agent = orchestrator.getAgent(agentName);
      if (!agent) {
        return res.status(404).json({ error: `Agent "${agentName}" not found.` });
      }
    } else {
      agent = orchestrator.getPrimaryAgent();
      if (!agent) {
        return res.status(500).json({ error: 'No primary agent configured.' });
      }
    }

    // Return immediately — streaming happens via Centrifugo
    res.json({ conversationId, messageId });

    // Process in background
    try {
      const response = await agent.processStream(message, history, messageId);
      const reply = response.finalAnswer || response.thought || '';
      history.push({ role: 'user', content: message });
      history.push({ role: 'assistant', content: reply });
    } catch (err: any) {
      logger.error(`[${agent.name}] Stream error:`, err);
    }
  } catch (error: any) {
    logger.error('Chat Stream API error:', error);
    if (!res.headersSent) {
      res.status(500).json({ error: error.message });
    }
  }
});

// ─── Config API ───────────────────────────────────────────────────────────────
/**
 * Gets legacy LLM config.
 * @param req Express request
 * @param res Express response
 * @returns {void}
 */
app.get('/api/config/llm', (req, res) => {
  res.json(config.llm);
});

/**
 * Updates legacy LLM config.
 * @param req Express request
 * @param res Express response
 * @returns {void}
 */
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

/**
 * Tests legacy LLM config.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
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

/**
 * Gets providers catalog.
 * @param req Express request
 * @param res Express response
 * @returns {void}
 */
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

/**
 * Updates provider config.
 * @param req Express request
 * @param res Express response
 * @returns {void}
 */
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

/**
 * Tests specific provider.
 * @param req Express request
 * @param res Express response
 * @returns {Promise<void>}
 */
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

/**
 * Sets active provider.
 * @param req Express request
 * @param res Express response
 * @returns {void}
 */
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
  const logger = new Logger('SERACore');
  logger.info('Shutting down SERA Core...');
  orchestrator.stopWatching();
  await lspManager.stopAll();
};

process.on('SIGTERM', shutdown);
process.on('SIGINT', shutdown);

if (process.env.NODE_ENV !== 'test') {
  const port = process.env.PORT || 3001;
  initDb().then(() => {
    app.listen(port, () => {
      const logger = new Logger('SERACore');
      logger.info(`SERA Core orchestrator listening at http://localhost:${port}`);
    });
  }).catch(err => {
    const logger = new Logger('SERACore');
    logger.error('Failed to start SERA Core:', err);
    process.exit(1);
  });
}
