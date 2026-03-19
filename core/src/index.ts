import express from 'express';
import cors from 'cors';
import path from 'path';
import { Logger } from './lib/logger.js';
import { IntercomService } from './intercom/IntercomService.js';
import { BridgeService } from './intercom/BridgeService.js';
import { SandboxManager } from './sandbox/SandboxManager.js';
import { Orchestrator } from './agents/Orchestrator.js';
import { MCPRegistry } from './mcp/registry.js';
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
import { createConfigRouter } from './routes/config.js';
import { createSchedulesRouter } from './routes/schedules.js';
import { createOpenAICompatRouter } from './routes/openai-compat.js';
import lspRouter, { lspManager } from './routes/lsp.js';
import { SessionStore } from './sessions/SessionStore.js';
import { IdentityService } from './auth/IdentityService.js';
import { MeteringService } from './metering/MeteringService.js';
import { MeteringEngine } from './metering/MeteringEngine.js';
import { AgentScheduler } from './metering/AgentScheduler.js';
import { TelegramAdapter } from './channels/adapters/TelegramAdapter.js';
import { DiscordAdapter } from './channels/adapters/DiscordAdapter.js';
import { WhatsAppAdapter } from './channels/adapters/WhatsAppAdapter.js';
import { initDb } from './lib/database.js';
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
import { pool } from './lib/database.js';

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
const mcpRegistry = MCPRegistry.getInstance();
const memoryManager = new MemoryManager();

const startServer = async () => {
  const toolExecutor = new ToolExecutor(skillRegistry, sandboxManager);

  // 1. Initialize Orchestrator & Skills
  orchestrator.loadTemplates(agentsDir);
  orchestrator.setIntercom(intercomService);
  orchestrator.setToolExecutor(toolExecutor);
  orchestrator.setSandboxManager(sandboxManager);

  registerBuiltinSkills(skillRegistry, memoryManager);
  mcpRegistry.getAllTools().then(async () => {
    const count = await skillRegistry.bridgeMCPTools(mcpRegistry);
    if (count > 0) logger.info(`Bridged ${count} MCP tool(s) as skills`);
  }).catch(() => {});

  const manifests = orchestrator.getAllManifests();
  for (const manifest of manifests) {
    const { AgentFactory } = await import('./agents/AgentFactory.js');
    const agent = AgentFactory.createAgent(manifest, undefined, intercomService);
    agent.setIntercom(intercomService);
    agent.setToolExecutor(toolExecutor);
    orchestrator.registerAgent(agent);
  }

  const circleRegistry = new CircleRegistry();
  const circlesDir = path.join(workspaceRoot, 'circles');
  const agentManifests = AgentManifestLoader.loadAllManifests(agentsDir);
  await circleRegistry.loadFromDirectory(circlesDir, agentManifests);

  bridgeService.init(intercomService, circleRegistry);
  intercomService.setBridgeService(bridgeService);

  // 2. Initialize Identity & Auth
  const identityService = new IdentityService();
  const authService = new AuthService();
  authService.registerPlugin(new ApiKeyProvider());
  const authMiddleware = createAuthMiddleware(identityService, authService);

  // 3. Initialize Shared State
  const sessionStore = new SessionStore();
  const meteringService = new MeteringService();
  const meteringEngine = new MeteringEngine();
  const agentScheduler = new AgentScheduler();
  
  orchestrator.setMetering(meteringEngine, agentScheduler);
  orchestrator.setIdentityService(identityService);
  orchestrator.watchAgentsDirectory(agentsDir);

  // 4. Initialize Secrets Manager
  try {
    SecretsManager.getInstance();
  } catch (err) {
    logger.warn('Failed to initialize SecretsManager:', err);
  }

  // 5. Setup Express App
  app.use(cors());
  app.use(express.json());

  app.get('/api/health', (req, res) => res.json({ status: 'ok', service: 'sera-core', timestamp: new Date().toISOString() }));

  // Mount Routers (Auth FIRST to avoid prefix interception)
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

  app.use('/api/agents', createAgentRouter(orchestrator, agentsDir));
  app.use('/api/circles', createCircleRouter(circleRegistry, circlesDir, () => AgentManifestLoader.loadAllManifests(agentsDir), orchestrator));
  app.use('/api/skills', createSkillsRouter(skillRegistry, orchestrator));
  app.use('/api/sessions', createSessionRouter(sessionStore));
  app.use('/api', createChatRouter(sessionStore, orchestrator));
  
  app.use('/v1/llm', createLlmProxyRouter(identityService, authService, meteringService));
  app.use('/api/agents', createHeartbeatRouter(orchestrator, identityService, authService));
  app.use('/api/budget', authMiddleware, createBudgetRouter());
  app.use('/api/audit', authMiddleware, createAuditRouter());
  app.use('/api', authMiddleware, createConfigRouter());
  app.use('/api/schedules', authMiddleware, createSchedulesRouter());
  app.use('/v1', createOpenAICompatRouter(orchestrator));
  app.use('/api/lsp', lspRouter);
  
  const agentRegistry = new AgentRegistry(pool);
  orchestrator.setRegistry(agentRegistry);
  const resourceImporter = new ResourceImporter(agentRegistry, workspaceRoot);
  const permissionService = new PermissionRequestService(agentRegistry);

  app.use('/api/registry', authMiddleware, createRegistryRouter(agentRegistry, resourceImporter));
  app.use('/api/agents', createLifecycleRouter(agentRegistry, orchestrator, sandboxManager, permissionService));
  app.use('/api/permission-requests', authMiddleware, createPermissionRouter(permissionService));

  // Story 3.5 — start Docker events listener after registry is ready
  await orchestrator.startDockerEventListener();

  // 6. External Adapters
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
    await initDb();
    
    // Perform auto-bootstrap
    const bootstrapService = new BootstrapService(agentRegistry, resourceImporter, workspaceRoot);
    await bootstrapService.ensureSeraInstantiated().catch(err => {
      logger.error('Sera auto-bootstrap failed:', err);
    });

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
