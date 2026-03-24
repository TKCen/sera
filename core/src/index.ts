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
import { createCirclesDbRouter } from './routes/circles-db.js';
import { createPipelinesRouter } from './routes/pipelines.js';
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
import { AuthService } from './auth/auth-service.js';
import { ApiKeyProvider } from './auth/api-key-provider.js';
import { OIDCAuthPlugin } from './auth/oidc-provider.js';
import { WebSessionStore } from './auth/web-session-store.js';
import { createAuthMiddleware } from './auth/authMiddleware.js';
import { createAuthRouter } from './routes/auth.js';
import { createSecretsRouter } from './routes/secrets.js';
import { SecretsManager } from './secrets/secrets-manager.js';
import { AgentRegistry } from './agents/registry.service.js';
import { ResourceImporter } from './agents/importer.service.js';
import { createRegistryRouter } from './routes/registry.js';
import { createLifecycleRouter, createPermissionRouter } from './routes/lifecycle.js';
import { createToolProxyRouter } from './routes/toolProxy.js';
import { PermissionRequestService } from './sandbox/PermissionRequestService.js';
import { ProviderRegistry } from './llm/ProviderRegistry.js';
import { LlmRouter } from './llm/LlmRouter.js';
import { DynamicProviderManager } from './llm/DynamicProviderManager.js';
import { CircuitBreakerService } from './llm/CircuitBreakerService.js';
import { createProvidersRouter, createSystemRouter } from './routes/providers.js';
import { createMeteringRouter } from './routes/metering.js';
import { createTasksRouter, pruneOldTaskResults } from './routes/tasks.js';
import { createKnowledgeRouter } from './routes/knowledge.js';
import { KnowledgeGitService } from './memory/KnowledgeGitService.js';
import { MemoryCompactionService } from './memory/MemoryCompactionService.js';
import { EmbeddingService } from './services/embedding.service.js';
import { AuditService } from './audit/AuditService.js';
import { ScheduleService } from './services/ScheduleService.js';
import { createDelegationRouter, expireOldDelegationTokens } from './routes/delegation.js';
import { createNotificationsRouter } from './routes/notifications.js';
import { NotificationService } from './channels/NotificationService.js';
import { PgBossService } from './lib/PgBossService.js';

const app = express();
const logger = new Logger('SERACore');

// ── Workspace Root ───────────────────────────────────────────────────────────
const workspaceRoot = process.env.WORKSPACE_DIR ?? path.resolve(import.meta.dirname, '..', '..');

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
if (process.env.OIDC_ISSUER_URL) {
  try {
    authService.registerPlugin(new OIDCAuthPlugin());
  } catch (err: unknown) {
    logger.warn(`OIDC provider disabled: ${(err as Error).message}`);
  }
} else {
  logger.warn('OIDC_ISSUER_URL not set — running in API-key-only mode');
}
const webSessionStore = new WebSessionStore();
const authMiddleware = createAuthMiddleware(identityService, authService, webSessionStore);
const sessionStore = new SessionStore();
const meteringService = new MeteringService();
const meteringEngine = new MeteringEngine();
const providerRegistry = new ProviderRegistry(
  process.env.PROVIDERS_CONFIG_PATH ?? '/app/config/providers.json'
);
const llmRouter = new LlmRouter(providerRegistry);
const dynamicProviderManager = new DynamicProviderManager(
  providerRegistry,
  process.env.DYNAMIC_PROVIDERS_CONFIG_PATH ?? '/app/config/dynamic_providers.json'
);
const circuitBreakerService = new CircuitBreakerService(llmRouter);
const agentScheduler = new AgentScheduler();
const permissionService = new PermissionRequestService(agentRegistry, intercomService);
const webhooksService = new WebhooksService(intercomService);
const resourceImporter = new ResourceImporter(agentRegistry, workspaceRoot);

// ── Initialization (Sync/Top-level parts) ────────────────────────────────────
orchestrator.loadTemplates(agentsDir);
orchestrator.setIntercom(intercomService);
orchestrator.setToolExecutor(toolExecutor);
orchestrator.setSandboxManager(sandboxManager);
orchestrator.setRegistry(agentRegistry);
sandboxManager.setAgentRegistry(agentRegistry);
orchestrator.setMetering(meteringEngine, agentScheduler);
orchestrator.setIdentityService(identityService);
orchestrator.setLlmRouter(llmRouter);

registerBuiltinSkills(skillRegistry, memoryManager);

const agentManifests = fs.existsSync(agentsDir)
  ? AgentManifestLoader.loadAllManifests(agentsDir)
  : [];
circleRegistry.loadFromDirectory(circlesDir, agentManifests).catch(() => {});

bridgeService.init(intercomService, circleRegistry);
intercomService.setBridgeService(bridgeService);

// ── Setup Express App ────────────────────────────────────────────────────────
app.use((req, _res, next) => {
  logger.info(`[REQUEST] ${req.method} ${req.url}`);
  next();
});

app.use(cors());
app.use(
  express.json({
    verify: (req: import('express').Request & { rawBody?: Buffer }, _res, buf) => {
      req.rawBody = buf;
    },
  })
);

app.get('/api/health', (req, res) =>
  res.json({ status: 'ok', service: 'sera-core', timestamp: new Date().toISOString() })
);

app.get('/api/health/detail', async (_req, res) => {
  const components: {
    name: string;
    status: 'healthy' | 'degraded' | 'unreachable';
    message?: string;
    latencyMs?: number;
  }[] = [];

  // Database check
  const dbStart = Date.now();
  try {
    await pool.query('SELECT 1');
    components.push({ name: 'database', status: 'healthy', latencyMs: Date.now() - dbStart });
  } catch (err: unknown) {
    components.push({ name: 'database', status: 'unreachable', message: (err as Error).message });
  }

  // Centrifugo check — ping the HTTP API
  const centrifugoUrl = process.env['CENTRIFUGO_API_URL'] ?? 'http://centrifugo:8000/api';
  const centrifugoStart = Date.now();
  try {
    const ctrl = new AbortController();
    const centrifugoTimer = setTimeout(() => ctrl.abort(), 3000);
    const centResp = await fetch(`${centrifugoUrl.replace(/\/api$/, '')}/health`, {
      signal: ctrl.signal,
    });
    clearTimeout(centrifugoTimer);
    components.push({
      name: 'centrifugo',
      status: centResp.ok ? 'healthy' : 'degraded',
      latencyMs: Date.now() - centrifugoStart,
    });
  } catch {
    components.push({
      name: 'centrifugo',
      status: 'unreachable',
      latencyMs: Date.now() - centrifugoStart,
    });
  }

  // Docker check — ping the Docker daemon
  const dockerStart = Date.now();
  try {
    await sandboxManager.ping();
    components.push({ name: 'docker', status: 'healthy', latencyMs: Date.now() - dockerStart });
  } catch {
    components.push({
      name: 'docker',
      status: 'unreachable',
      latencyMs: Date.now() - dockerStart,
    });
  }

  // Qdrant check — ping the REST API
  const qdrantUrl = process.env['QDRANT_URL'] ?? 'http://localhost:6333';
  const qdrantStart = Date.now();
  try {
    const ctrl = new AbortController();
    const qdrantTimer = setTimeout(() => ctrl.abort(), 3000);
    const qdResp = await fetch(`${qdrantUrl}/healthz`, { signal: ctrl.signal });
    clearTimeout(qdrantTimer);
    components.push({
      name: 'qdrant',
      status: qdResp.ok ? 'healthy' : 'degraded',
      latencyMs: Date.now() - qdrantStart,
    });
  } catch {
    components.push({
      name: 'qdrant',
      status: 'unreachable',
      latencyMs: Date.now() - qdrantStart,
    });
  }

  // pg-boss check — verify the job queue is running
  const pgBossStart = Date.now();
  try {
    PgBossService.getInstance().getBoss();
    components.push({ name: 'pg-boss', status: 'healthy', latencyMs: Date.now() - pgBossStart });
  } catch {
    components.push({
      name: 'pg-boss',
      status: 'unreachable',
      message: 'pg-boss not started',
      latencyMs: Date.now() - pgBossStart,
    });
  }

  // Squid egress proxy check
  const squidHost = process.env['SQUID_HOST'] ?? 'sera-egress-proxy';
  const squidPort = process.env['SQUID_PORT'] ?? '3128';
  const squidStart = Date.now();
  try {
    const ctrl = new AbortController();
    const squidTimer = setTimeout(() => ctrl.abort(), 3000);
    await fetch(`http://${squidHost}:${squidPort}`, { signal: ctrl.signal });
    clearTimeout(squidTimer);
    // Squid returns 400/403 for non-proxy requests, but a response means it's alive
    components.push({ name: 'squid', status: 'healthy', latencyMs: Date.now() - squidStart });
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.name : '';
    // AbortError means timeout; ECONNREFUSED means down
    components.push({
      name: 'squid',
      status: msg === 'AbortError' ? 'degraded' : 'unreachable',
      latencyMs: Date.now() - squidStart,
    });
  }

  // Agent stats
  let agentStats = { total: 0, running: 0, stopped: 0, errored: 0 };
  try {
    const instances = await agentRegistry.listInstances();
    agentStats = {
      total: instances.length,
      running: instances.filter((i) => i.status === 'running').length,
      stopped: instances.filter((i) => i.status === 'stopped').length,
      errored: instances.filter((i) => i.status === 'error').length,
    };
  } catch {
    // non-fatal
  }

  const overall = components.every((c) => c.status === 'healthy')
    ? 'healthy'
    : components.some((c) => c.status === 'unreachable')
      ? 'unhealthy'
      : 'degraded';

  res.json({ status: overall, components, agentStats, timestamp: new Date().toISOString() });
});

// Mount Routers
// Auth: public endpoints first (no authMiddleware), then protected
const { publicAuthRouter, protectedAuthRouter } = createAuthRouter(webSessionStore);
app.use('/api/auth', publicAuthRouter);
app.use('/api/auth', authMiddleware, protectedAuthRouter);
app.use('/api/secrets', authMiddleware, createSecretsRouter());

const sandboxRouter = createSandboxRouter(sandboxManager, (name) =>
  agentManifests.find((m) => m.metadata.name === name)
);
app.use('/api/sandbox', sandboxRouter);

const intercomRouter = createIntercomRouter(
  intercomService,
  (name) => agentManifests.find((m) => m.metadata.name === name),
  bridgeService
);
app.use('/api/intercom', intercomRouter);

app.use('/api/agents', createHeartbeatRouter(orchestrator, identityService, authService));
app.use(
  '/api/agents',
  authMiddleware,
  createLifecycleRouter(agentRegistry, orchestrator, sandboxManager, permissionService)
);
app.use('/api/agents', authMiddleware, createAgentRouter(orchestrator, agentRegistry));
app.use(
  '/v1/tools',
  createToolProxyRouter(identityService, authService, permissionService, agentRegistry)
);

// ── Convenience routes for the web UI ────────────────────────────────────────
// GET /api/tools — list executable tools with security metadata (used by AgentForm)
app.get('/api/tools', authMiddleware, (_req, res) => {
  const tools = skillRegistry.listTools();
  const manifests = orchestrator.getAllManifests();
  const enriched = tools.map((tool) => {
    const usedBy: string[] = [];
    for (const manifest of manifests) {
      const allowed = manifest.tools?.allowed ?? [];
      if (allowed.includes(tool.id) || allowed.includes('*')) {
        usedBy.push(manifest.metadata.name);
      }
    }
    return { ...tool, usedBy };
  });
  res.json(enriched);
});

// GET /api/templates — list agent templates from the DB (used by AgentForm)
app.get('/api/templates', authMiddleware, async (_req, res) => {
  try {
    const templates = await agentRegistry.listTemplates();
    res.json(templates);
  } catch (err: unknown) {
    res.status(500).json({ error: (err as Error).message });
  }
});

// GET /api/rt/token — issue a Centrifugo connection token for the web client
app.get('/api/rt/token', authMiddleware, async (_req, res) => {
  try {
    const token = await intercomService.generateConnectionToken('web-operator');
    // Decode exp claim from the JWT payload (second segment, base64url-encoded)
    const payloadB64 = token.split('.')[1] ?? '';
    const payload = JSON.parse(Buffer.from(payloadB64, 'base64url').toString('utf-8')) as {
      exp?: number;
    };
    const expiresAt = payload.exp ?? Math.floor(Date.now() / 1000) + 86400;
    res.json({ token, expiresAt });
  } catch (err: unknown) {
    res.status(500).json({ error: (err as Error).message });
  }
});

app.use(
  '/api/circles',
  authMiddleware,
  createCircleRouter(
    circleRegistry,
    circlesDir,
    () => AgentManifestLoader.loadAllManifests(agentsDir),
    orchestrator
  )
);
app.use('/api/circles', authMiddleware, createCirclesDbRouter(orchestrator));
app.use('/api/pipelines', authMiddleware, createPipelinesRouter(orchestrator));
app.use('/api/skills', authMiddleware, createSkillsRouter(skillRegistry, orchestrator, pool));
app.use('/api/memory', authMiddleware, createMemoryRouter(memoryManager));
app.use('/api/sessions', authMiddleware, createSessionRouter(sessionStore));
app.use('/api', authMiddleware, createChatRouter(sessionStore, orchestrator, agentRegistry));

app.use(
  '/v1/llm',
  createLlmProxyRouter(
    identityService,
    authService,
    meteringService,
    llmRouter,
    circuitBreakerService,
    pool,
    orchestrator
  )
);
app.use('/api/budget', authMiddleware, createBudgetRouter(meteringService));
app.use(
  '/api/providers',
  authMiddleware,
  createProvidersRouter(llmRouter, circuitBreakerService, dynamicProviderManager)
);
app.use('/api/system', authMiddleware, createSystemRouter(circuitBreakerService));
app.use('/api/metering', authMiddleware, createMeteringRouter(meteringService));
app.use('/api/audit', authMiddleware, createAuditRouter());
app.use('/api/permission-requests', authMiddleware, createPermissionRouter(permissionService));
app.use('/api', authMiddleware, createConfigRouter());
app.use('/api/schedules', authMiddleware, createSchedulesRouter());
app.use('/v1', createOpenAICompatRouter(orchestrator));
app.use('/api/lsp', lspRouter);
app.use('/api/federation', createFederationRouter());
app.use('/api/webhooks', createWebhooksRouter(webhooksService, authMiddleware));

app.use(
  '/api/registry',
  authMiddleware,
  createRegistryRouter(agentRegistry, resourceImporter, orchestrator)
);
app.use('/api/mcp-servers', authMiddleware, createMCPRouter(mcpRegistry, skillRegistry));
app.use('/api/agents/:id/tasks', createTasksRouter(intercomService));
app.use('/api/knowledge', authMiddleware, createKnowledgeRouter(llmRouter));

// Epic 17 — Delegation & Service Identity
const delegationRouter = createDelegationRouter(intercomService);
app.use('/api/delegation', authMiddleware, delegationRouter);
app.use('/api', authMiddleware, delegationRouter);

// Epic 18 — Integration Channels
const { publicRouter: notifPublicRouter, protectedRouter: notifProtectedRouter } =
  createNotificationsRouter();
app.use('/api/notifications', notifPublicRouter);
app.use('/api/notifications', authMiddleware, notifProtectedRouter);
app.use('/api/channels', authMiddleware, notifProtectedRouter);

// Global Error Handler
app.use(
  (
    err: unknown,
    _req: import('express').Request,
    res: import('express').Response,
    _next: import('express').NextFunction
  ) => {
    logger.error('Unhandled API Error:', err);
    const status = (err as { status?: number }).status || 500;
    res.status(status).json({
      error: err instanceof Error ? err.message : String(err),
    });
  }
);

const startServer = async () => {
  mcpRegistry.setIntercom(intercomService);

  const { SeraMCPServer } = await import('./mcp/SeraMCPServer.js');
  const seraMcpServer = new SeraMCPServer(orchestrator);
  // For simplicity in this story, we bridge it directly since it's in-process.
  // In a full implementation, we'd use a local transport.
  await mcpRegistry.registerSeraCoreTools(seraMcpServer);

  mcpRegistry.onRegister((name) => {
    skillRegistry
      .bridgeMCPToolsForServer(name, mcpRegistry)
      .then((count) => {
        if (count > 0) logger.info(`Bridged ${count} new MCP tool(s) from "${name}"`);
      })
      .catch(() => {});
  });

  mcpRegistry.onUnregister((name) => {
    skillRegistry.unregisterByPrefix(`${name}/`);
    logger.info(`Removed bridged skills for MCP server "${name}"`);
  });

  await mcpRegistry
    .loadFromDirectory(mcpServersDir)
    .catch((err) => logger.error('Failed to load MCP servers:', err));
  mcpRegistry.watchDirectory(mcpServersDir);

  const manifests = orchestrator.getAllManifests();
  for (const manifest of manifests) {
    const { AgentFactory } = await import('./agents/AgentFactory.js');
    const agent = AgentFactory.createAgent(manifest, undefined, intercomService, llmRouter);
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

  // Hydrate provider API keys from encrypted secrets store (if SECRETS_MASTER_KEY is set)
  await providerRegistry.hydrateSecrets().catch((err) => {
    logger.warn('Could not hydrate provider secrets (SECRETS_MASTER_KEY may not be set):', err);
  });

  // Start dynamic provider polling (file-based config, no DB needed but placed here
  // for consistent startup ordering — after migrations, before service consumers)
  await dynamicProviderManager.start();

  // Epic 11 — Initialize Audit Trail
  const auditService = AuditService.getInstance();
  await auditService
    .initialize()
    .catch((err) => logger.error('Failed to initialize AuditService:', err));

  // Start shared pg-boss instance (used by ScheduleService, NotificationService,
  // and optionally MemoryCompactionService — one connection pool, one polling loop)
  const sharedBoss = process.env.DATABASE_URL
    ? await PgBossService.getInstance()
        .start(process.env.DATABASE_URL)
        .catch((err: unknown) => {
          logger.error('Failed to start PgBossService:', err);
          return null;
        })
    : null;

  // Epic 11 — Initialize Schedule Service
  if (sharedBoss) {
    const scheduleService = ScheduleService.getInstance();
    scheduleService.setOrchestrator(orchestrator);
    await scheduleService
      .start(sharedBoss)
      .catch((err) => logger.error('Failed to start ScheduleService:', err));
  }

  // Story 6.2 — load skills from library
  const { SkillLibrary } = await import('./skills/SkillLibrary.js');
  const skillLibrary = SkillLibrary.getInstance(pool);
  skillLibrary.setIntercom(intercomService);
  const skillStats = await skillLibrary.loadSkills().catch((err) => {
    logger.error('Failed to load Skill Library:', err);
    return { updated: 0, skipped: 0, errors: [err.message] };
  });
  logger.info(
    `Skill Library loaded: ${skillStats.updated} skills/packages, ${skillStats.skipped} skipped, ${skillStats.errors.length} errors`
  );

  skillLibrary.watchSkills();

  // Epic 8 — warm up embedding service and init system knowledge repo
  await EmbeddingService.getInstance().warmup();
  await KnowledgeGitService.getInstance()
    .initSystemRepo()
    .catch((err) => logger.warn('Failed to init system knowledge repo:', err));
  if (sharedBoss && MemoryCompactionService.isEnabled()) {
    await MemoryCompactionService.getInstance()
      .start(sharedBoss)
      .catch((err) => logger.warn('MemoryCompactionService failed to start:', err));
  }

  await orchestrator.startDockerEventListener();

  const channelOptions = {
    rateLimitWindow: config.channels.rateLimit.windowMs,
    maxMessagesPerWindow: config.channels.rateLimit.maxMessages,
  };
  if (config.channels.telegram.token) {
    new TelegramAdapter(config.channels.telegram.token, orchestrator, sessionStore, channelOptions)
      .start()
      .catch((err) => logger.error('Failed to start Telegram adapter:', err));
  }
  if (config.channels.discord.token) {
    new DiscordAdapter(config.channels.discord.token, orchestrator, sessionStore, channelOptions)
      .start()
      .catch((err) => logger.error('Failed to start Discord adapter:', err));
  }
  if (config.channels.whatsapp.token && config.channels.whatsapp.phoneNumberId) {
    new WhatsAppAdapter(
      config.channels.whatsapp.token,
      config.channels.whatsapp.phoneNumberId,
      orchestrator,
      sessionStore,
      channelOptions
    )
      .start()
      .catch((err) => logger.error('Failed to start WhatsApp adapter:', err));
  }

  if (process.env.NODE_ENV !== 'test') {
    const port = process.env.PORT || 3001;
    pruneOldTaskResults().catch((err) => logger.warn('Task result pruning error:', err));
    setInterval(
      () => {
        pruneOldTaskResults().catch((err) => logger.warn('Task result pruning error:', err));
      },
      60 * 60 * 1_000
    );

    // Epic 17 — Expire delegation tokens every 5 minutes
    expireOldDelegationTokens().catch(() => {});
    setInterval(
      () => {
        expireOldDelegationTokens().catch(() => {});
      },
      5 * 60 * 1_000
    );

    // Epic 18 — Start notification service and wire permission hook
    if (sharedBoss) {
      const notificationService = NotificationService.getInstance();
      notificationService.setPermissionService(permissionService);
      notificationService.setOrchestrator(orchestrator);
      notificationService.setSessionStore(sessionStore);
      await notificationService
        .start(sharedBoss)
        .catch((err: unknown) => logger.error('NotificationService failed to start:', err));

      permissionService.setOnRequestCreated((req) => {
        notificationService.dispatchEvent(
          'permission.requested',
          `Permission Request: ${req.dimension}=${req.value}`,
          `Agent **${req.agentName}** (${req.agentId}) requests ${req.dimension} access to \`${req.value}\`.\n${req.reason ?? ''}`,
          'warning',
          {
            requestId: req.requestId,
            agentId: req.agentId,
            dimension: req.dimension,
            value: req.value,
          },
          { requestId: req.requestId, requestType: 'permission' }
        );
      });
    }

    app.listen(port, () => logger.info(`SERA Core running on port ${port}`));
  }
};

const shutdown = async () => {
  logger.info('Shutting down SERA Core...');
  orchestrator.stopWatching();
  await lspManager.stopAll();
  await PgBossService.getInstance().stop();
};
process.on('SIGTERM', shutdown);
process.on('SIGINT', shutdown);

const isMainModule =
  import.meta.url === `file://${process.argv[1]}` ||
  (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(import.meta.filename));

if (process.env.NODE_ENV !== 'test' && isMainModule) {
  startServer().catch((err) => {
    logger.error('Failed to start server:', err);
    process.exit(1);
  });
}

export { app, startServer };
