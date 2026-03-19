import express from 'express';
import cors from 'cors';
import path from 'path';
import fs from 'node:fs';
import { Logger } from './lib/logger.js';
import { IntercomService } from './intercom/IntercomService.js';
import { BridgeService } from './intercom/BridgeService.js';
import { SandboxManager } from './sandbox/SandboxManager.js';
import { Orchestrator } from './agents/Orchestrator.js';
import { MCPRegistry } from './mcp/registry.js';
import { MCPServerManager } from './mcp/MCPServerManager.js';
import { MemoryManager } from './memory/manager.js';
import { SkillRegistry } from './skills/SkillRegistry.js';
import { registerBuiltinSkills } from './skills/builtins/index.js';
import { ToolExecutor } from './tools/ToolExecutor.js';
import { CircleRegistry } from './circles/CircleRegistry.js';
import { AgentManifestLoader } from './agents/manifest/AgentManifestLoader.js';
import { createSandboxRouter } from './routes/sandbox.js';
import { createIntercomRouter } from './routes/intercom.js';
import { createAgentRouter } from './routes/agents.js';
import { createCircleRouter } from './routes/circles.js';
import { createSkillsRouter } from './routes/skills.js';
import { createSessionRouter } from './routes/sessions.js';
import { createChatRouter } from './routes/chat.js';
import { createLlmProxyRouter } from './routes/llmProxy.js';
import { createHeartbeatRouter } from './routes/heartbeat.js';
import { createBudgetRouter } from './routes/budget.js';
import { createAuditRouter } from './routes/audit.js';
import { createFederationRouter } from './routes/federation.js';
import { createConfigRouter } from './routes/config.js';
import { createSchedulesRouter } from './routes/schedules.js';
import { createOpenAICompatRouter } from './routes/openai-compat.js';
import { WebhooksService } from './intercom/WebhooksService.js';
import { createWebhooksRouter } from './routes/webhooks.js';
import { createMemoryRouter } from './routes/memory.js';
import { createMCPRouter } from './routes/mcp.js';
import lspRouter, { lspManager } from './routes/lsp.js';
import { SessionStore } from './sessions/SessionStore.js';
import { IdentityService } from './auth/IdentityService.js';
import { MeteringService } from './metering/MeteringService.js';
import { MeteringEngine } from './metering/MeteringEngine.js';
import { AgentScheduler } from './metering/AgentScheduler.js';
import { TelegramAdapter } from './channels/adapters/TelegramAdapter.js';
import { DiscordAdapter } from './channels/adapters/DiscordAdapter.js';
import { WhatsAppAdapter } from './channels/adapters/WhatsAppAdapter.js';
import { initDb, pool } from './lib/database.js';
import { config } from './lib/config.js';
import { OpenAIProvider } from './lib/llm/OpenAIProvider.js';
import { AuthService } from './auth/auth-service.js';
import { ApiKeyProvider } from './auth/api-key-provider.js';
import { createAuthMiddleware } from './auth/authMiddleware.js';
import { createAuthRouter } from './routes/auth.js';
import { createSecretsRouter } from './routes/secrets.js';
import { SecretsManager } from './secrets/secrets-manager.js';
import { AgentRegistry } from './agents/registry.service.js';
import { ResourceImporter } from './agents/importer.service.js';
import { BootstrapService } from './agents/bootstrap.service.js';
import { createRegistryRouter } from './routes/registry.js';
import { createLifecycleRouter, createPermissionRouter } from './routes/lifecycle.js';
import { PermissionRequestService } from './sandbox/PermissionRequestService.js';
import { LiteLLMClient } from './llm/LiteLLMClient.js';
import { CircuitBreakerService } from './llm/CircuitBreakerService.js';
import { createProvidersRouter, createSystemRouter } from './routes/providers.js';
import { createMeteringRouter } from './routes/metering.js';
import { createTasksRouter, pruneOldTaskResults } from './routes/tasks.js';
import { createKnowledgeRouter } from './routes/knowledge.js';
import { KnowledgeGitService } from './memory/KnowledgeGitService.js';
import { MemoryCompactionService } from './memory/MemoryCompactionService.js';
import { EmbeddingService } from './services/embedding.service.js';

const app = express();
const logger = new Logger('SERACore');

// ── Workspace Root ───────────────────────────────────────────────────────────
const workspaceRoot = process.env.WORKSPACE_DIR
  ?? path.resolve(import.meta.dirname, '..', '..');

// Global instances
const intercomService = new IntercomService();
const bridgeService = new BridgeService();
const sandboxManager = new SandboxManager();
const orchestrator = new Orchestrator();
const skillRegistry = new SkillRegistry();
const agentsDir = path.join(workspaceRoot, 'agents');
const mcpServersDir = path.join(workspaceRoot, 'mcp-servers');
const mcpRegistry = MCPRegistry.getInstance();
const mcpServerManager = new MCPServerManager(sandboxManager);
mcpRegistry.setManager(mcpServerManager);
mcpRegistry.setIntercom(intercomService);

const memoryManager = new MemoryManager();
const toolExecutor = new ToolExecutor(skillRegistry, sandboxManager);
const circleRegistry = new CircleRegistry();
const circlesDir = path.join(workspaceRoot, 'circles');
const agentRegistry = new AgentRegistry(pool);
const identityService = new IdentityService();
const authService = new AuthService();
authService.registerPlugin(new ApiKeyProvider());
const authMiddleware = createAuthMiddleware(identityService, authService);
const sessionStore = new SessionStore();
const meteringService = new MeteringService();
const meteringEngine = new MeteringEngine();
const liteLLMClient = new LiteLLMClient();
const circuitBreakerService = new CircuitBreakerService(liteLLMClient);
const agentScheduler = new AgentScheduler();
const permissionService = new PermissionRequestService(agentRegistry);
const webhooksService = new WebhooksService(intercomService);
const resourceImporter = new ResourceImporter(agentRegistry, workspaceRoot);

// ── Initialization (Sync/Top-level parts) ────────────────────────────────────
orchestrator.loadTemplates(agentsDir);
orchestrator.setIntercom(intercomService);
orchestrator.setToolExecutor(toolExecutor);
orchestrator.setSandboxManager(sandboxManager);
orchestrator.setRegistry(agentRegistry);
orchestrator.setMetering(meteringEngine, agentScheduler);
orchestrator.setIdentityService(identityService);

registerBuiltinSkills(skillRegistry, memoryManager);

const agentManifests = fs.existsSync(agentsDir) ? AgentManifestLoader.loadAllManifests(agentsDir) : [];
circleRegistry.loadFromDirectory(circlesDir, agentManifests).catch(() => {});

bridgeService.init(intercomService, circleRegistry);
intercomService.setBridgeService(bridgeService);

// ── Setup Express App ────────────────────────────────────────────────────────
app.use(cors());
app.use(express.json({
  verify: (req: any, res, buf) => {
    req.rawBody = buf;
  }
}));

app.get('/api/health', (req, res) => res.json({ status: 'ok', service: 'sera-core', timestamp: new Date().toISOString() }));

// Mount Routers
app.use('/api/auth', authMiddleware, createAuthRouter());
app.use('/api/secrets', authMiddleware, createSecretsRouter());

const sandboxRouter = createSandboxRouter(sandboxManager, (name) => 
  agentManifests.find(m => m.metadata.name === name));
app.use('/api/sandbox', sandboxRouter);

const intercomRouter = createIntercomRouter(
  intercomService, 
  (name) => agentManifests.find(m => m.metadata.name === name), 
  bridgeService
);
app.use('/api/intercom', intercomRouter);

app.use('/api/agents', createHeartbeatRouter(orchestrator, identityService, authService));
app.use('/api/agents', createLifecycleRouter(agentRegistry, orchestrator, sandboxManager, permissionService));
app.use('/api/agents', createAgentRouter(orchestrator, agentsDir));

app.use('/api/circles', createCircleRouter(circleRegistry, circlesDir, () => AgentManifestLoader.loadAllManifests(agentsDir), orchestrator));
app.use('/api/skills', createSkillsRouter(skillRegistry, orchestrator, pool));
app.use('/api/memory', createMemoryRouter(memoryManager));
app.use('/api/sessions', createSessionRouter(sessionStore));
app.use('/api', createChatRouter(sessionStore, orchestrator));

app.use('/v1/llm', createLlmProxyRouter(identityService, authService, meteringService, liteLLMClient, circuitBreakerService, pool, orchestrator));
app.use('/api/budget', authMiddleware, createBudgetRouter(meteringService));
app.use('/api/providers', authMiddleware, createProvidersRouter(liteLLMClient, circuitBreakerService));
app.use('/api/system', authMiddleware, createSystemRouter(circuitBreakerService));
app.use('/api/metering', authMiddleware, createMeteringRouter(meteringService));
app.use('/api/audit', authMiddleware, createAuditRouter());
app.use('/api', authMiddleware, createConfigRouter());
app.use('/api/schedules', authMiddleware, createSchedulesRouter());
app.use('/v1', createOpenAICompatRouter(orchestrator));
app.use('/api/lsp', lspRouter);
app.use('/api/federation', createFederationRouter());
app.use('/api/webhooks', createWebhooksRouter(webhooksService, authMiddleware));

app.use('/api/registry', authMiddleware, createRegistryRouter(agentRegistry, resourceImporter));
app.use('/api/mcp-servers', authMiddleware, createMCPRouter(mcpRegistry, skillRegistry));
app.use('/api/agents/:id/tasks', createTasksRouter(intercomService));
app.use('/api/knowledge', authMiddleware, createKnowledgeRouter());

const startServer = async () => {
  mcpRegistry.setIntercom(intercomService);

  const { SeraMCPServer } = await import('./mcp/SeraMCPServer.js');
  const seraMcpServer = new SeraMCPServer(orchestrator);
  // For simplicity in this story, we bridge it directly since it's in-process.
  // In a full implementation, we'd use a local transport.
  await mcpRegistry.registerSeraCoreTools(seraMcpServer);

  mcpRegistry.onRegister((name) => {
    skillRegistry.bridgeMCPToolsForServer(name, mcpRegistry).then(count => {
      if (count > 0) logger.info(`Bridged ${count} new MCP tool(s) from "${name}"`);
    }).catch(() => {});
  });

  mcpRegistry.onUnregister((name) => {
    skillRegistry.unregisterByPrefix(`${name}/`);
    logger.info(`Removed bridged skills for MCP server "${name}"`);
  });

  await mcpRegistry.loadFromDirectory(mcpServersDir).catch(err => logger.error('Failed to load MCP servers:', err));
  mcpRegistry.watchDirectory(mcpServersDir);

  const manifests = orchestrator.getAllManifests();
  for (const manifest of manifests) {
    const { AgentFactory } = await import('./agents/AgentFactory.js');
    const agent = AgentFactory.createAgent(manifest, undefined, intercomService);
    agent.setIntercom(intercomService);
    agent.setToolExecutor(toolExecutor);
    orchestrator.registerAgent(agent);
  }

  orchestrator.watchAgentsDirectory(agentsDir);

  try {
    SecretsManager.getInstance();
  } catch (err) {
    logger.warn('Failed to initialize SecretsManager:', err);
  }

  if (process.env.NODE_ENV !== 'test') {
    await initDb();
  }

  // Story 6.2 — load skills from library
  const { SkillLibrary } = await import('./skills/SkillLibrary.js');
  const skillLibrary = SkillLibrary.getInstance(pool);
  skillLibrary.setIntercom(intercomService);
  const skillStats = await skillLibrary.loadSkills().catch(err => {
    logger.error('Failed to load Skill Library:', err);
    return { updated: 0, skipped: 0, errors: [err.message] };
  });
  logger.info(`Skill Library loaded: ${skillStats.updated} skills/packages, ${skillStats.skipped} skipped, ${skillStats.errors.length} errors`);
  
  skillLibrary.watchSkills();

  // Epic 8 — warm up embedding service and init system knowledge repo
  await EmbeddingService.getInstance().warmup();
  await KnowledgeGitService.getInstance().initSystemRepo().catch(err =>
    logger.warn('Failed to init system knowledge repo:', err),
  );
  if (process.env.DATABASE_URL) {
    await MemoryCompactionService.getInstance().start(process.env.DATABASE_URL).catch(err =>
      logger.warn('MemoryCompactionService failed to start:', err),
    );
  }

  await orchestrator.startDockerEventListener();

  const channelOptions = { rateLimitWindow: config.channels.rateLimit.windowMs, maxMessagesPerWindow: config.channels.rateLimit.maxMessages };
  if (config.channels.telegram.token) {
    new TelegramAdapter(config.channels.telegram.token, orchestrator, sessionStore, channelOptions).start().catch(err => logger.error('Failed to start Telegram adapter:', err));
  }
  if (config.channels.discord.token) {
    new DiscordAdapter(config.channels.discord.token, orchestrator, sessionStore, channelOptions).start().catch(err => logger.error('Failed to start Discord adapter:', err));
  }
  if (config.channels.whatsapp.token && config.channels.whatsapp.phoneNumberId) {
    new WhatsAppAdapter(config.channels.whatsapp.token, config.channels.whatsapp.phoneNumberId, orchestrator, sessionStore, channelOptions).start().catch(err => logger.error('Failed to start WhatsApp adapter:', err));
  }

  if (process.env.NODE_ENV !== 'test') {
    const port = process.env.PORT || 3001;
    const bootstrapService = new BootstrapService(agentRegistry, resourceImporter, workspaceRoot);
    await bootstrapService.ensureSeraInstantiated().catch(err => {
      logger.error('Sera auto-bootstrap failed:', err);
    });

    pruneOldTaskResults().catch(err => logger.warn('Task result pruning error:', err));
    setInterval(() => {
      pruneOldTaskResults().catch(err => logger.warn('Task result pruning error:', err));
    }, 60 * 60 * 1_000);

    app.listen(port, () => logger.info(`SERA Core running on port ${port}`));
  }
};

const shutdown = async () => {
  logger.info('Shutting down SERA Core...');
  orchestrator.stopWatching();
  await lspManager.stopAll();
};
process.on('SIGTERM', shutdown);
process.on('SIGINT', shutdown);

const isMainModule = import.meta.url === `file://${process.argv[1]}` ||
                   (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(import.meta.filename));

if (process.env.NODE_ENV !== 'test' && isMainModule) {
  startServer().catch(err => {
    logger.error('Failed to start server:', err);
    process.exit(1);
  });
}

export { app, startServer };
