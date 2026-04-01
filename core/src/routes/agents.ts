/**
 * Agent Management Routes
 *
 * Instance CRUD, lifecycle, and template listing.
 */

import { Router } from 'express';
import { asyncHandler } from '../middleware/asyncHandler.js';
import type { Orchestrator } from '../agents/index.js';
import type { AgentRegistry } from '../agents/index.js';
import { AgentManifestLoader } from '../agents/index.js';
import type { AgentManifest } from '../agents/index.js';
import { AgentFactory } from '../agents/index.js';
import { IdentityService } from '../agents/index.js';
import { ContextAssembler, type ContextAssemblyEvent } from '../llm/index.js';
import { pool as dbPool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('AgentRouter');

export function createAgentRouter(orchestrator: Orchestrator, agentRegistry: AgentRegistry) {
  const router = Router();

  // ── List all agent instances (primary endpoint for the web UI) ────────────
  /**
   * Returns all agent instances from the DB, enriched with template metadata
   * and live orchestrator status.
   */
  router.get(
    '/',
    asyncHandler(async (_req, res) => {
      const instances = await agentRegistry.listInstances();
      const liveAgents = new Map(orchestrator.listAgents().map((a) => [a.id, a]));

      const enriched = await Promise.all(
        instances.map(async (inst) => {
          const template = await agentRegistry.getTemplate(inst.template_ref);
          const live = liveAgents.get(inst.id);
          return {
            id: inst.id,
            name: inst.name,
            display_name: inst.display_name ?? template?.display_name,
            template_ref: inst.template_ref,
            status: live?.status ?? inst.status,
            circle: inst.circle,
            lifecycle_mode: inst.lifecycle_mode,
            icon: template?.spec?.identity?.icon ?? template?.spec?.icon,
            sandbox_boundary:
              (inst as unknown as Record<string, unknown>).sandbox_boundary ??
              template?.spec?.sandboxBoundary,
            created_at: inst.created_at,
            updated_at: inst.updated_at,
          };
        })
      );

      res.json(enriched);
    })
  );

  // ── List all agent templates ───────────────────────────────────────────────
  router.get(
    '/templates',
    asyncHandler(async (_req, res) => {
      const templates = await agentRegistry.listTemplates();
      res.json(templates);
    })
  );

  // ── List all agent instances (raw DB) ─────────────────────────────────────
  router.get(
    '/instances',
    asyncHandler(async (req, res) => {
      const circle = req.query.circle as string | undefined;
      const status = req.query.status as string | undefined;
      const instances = await agentRegistry.listInstances({
        ...(circle ? { circle } : {}),
        ...(status ? { status } : {}),
      });
      res.json(instances);
    })
  );

  // ── Create a new agent instance ────────────────────────────────────────────
  /**
   * Creates a new agent instance from a template.
   * POST /api/agents/instances
   * { templateRef: string, name: string, displayName?: string, circle?: string,
   *   overrides?: object, lifecycleMode?: string, start?: boolean }
   */
  router.post(
    '/instances',
    asyncHandler(async (req, res) => {
      const { templateRef, name, displayName, circle, overrides, lifecycleMode, start } = req.body;

      // Support legacy field name
      const templateName = templateRef ?? req.body.templateName;

      if (!templateName || !name) {
        res.status(400).json({ error: 'templateRef and name are required' });
        return;
      }

      // Check for duplicate instance name
      const existingInstance = await agentRegistry.getInstanceByName(name);
      if (existingInstance) {
        res.status(409).json({ error: `Agent instance with name "${name}" already exists` });
        return;
      }

      // Verify template exists
      const template = await agentRegistry.getTemplate(templateName);
      if (!template) {
        res.status(404).json({ error: `Template "${templateName}" not found` });
        return;
      }

      // Create instance in DB via registry (not AgentFactory, which may not have registry)
      const instance = await agentRegistry.createInstance({
        name,
        displayName,
        templateRef: templateName,
        circle,
        overrides,
        lifecycleMode,
      });

      // Optionally start the instance (spawn Docker container)
      if (start !== false) {
        try {
          await orchestrator.startInstance(instance.id);
        } catch (startErr) {
          // Instance created but failed to start — return it with error status
          logger.error(`Instance ${instance.id} created but failed to start:`, startErr);
        }
      }

      // Re-fetch to get updated status
      const updated = await agentRegistry.getInstance(instance.id);
      res.status(201).json(updated ?? instance);
    })
  );

  // ── Spawn ephemeral agent (#334) ─────────────────────────────────────────────
  /**
   * POST /api/agents/spawn-ephemeral
   * One-shot: create instance → spawn container → execute task → return result.
   */
  router.post(
    '/spawn-ephemeral',
    asyncHandler(async (req, res) => {
      const {
        templateRef,
        task,
        parentInstanceId,
        ttlMinutes = 30,
        overrides,
        additionalMounts,
        async: asyncMode,
      } = req.body as {
        templateRef: string;
        task: string;
        parentInstanceId?: string;
        ttlMinutes?: number;
        overrides?: Record<string, unknown>;
        additionalMounts?: Array<{ hostPath: string; containerPath: string; mode?: 'ro' | 'rw' }>;
        async?: boolean;
      };

      if (!templateRef || !task) {
        res.status(400).json({ error: 'templateRef and task are required' });
        return;
      }

      // Verify template exists
      const template = await agentRegistry.getTemplate(templateRef);
      if (!template) {
        res.status(404).json({ error: `Template "${templateRef}" not found` });
        return;
      }

      const shortId = crypto.randomUUID().substring(0, 8);
      const instanceName = `${templateRef}-ephemeral-${shortId}`;
      const startTime = Date.now();

      // Create ephemeral instance
      const createData: Parameters<typeof agentRegistry.createInstance>[0] = {
        name: instanceName,
        templateRef,
        lifecycleMode: 'ephemeral',
      };
      if (parentInstanceId) createData.parentInstanceId = parentInstanceId;
      if (overrides) createData.overrides = overrides;
      const instance = await agentRegistry.createInstance(createData);

      try {
        // Start the container (spawn + readiness poll)
        await orchestrator.startInstance(instance.id, parentInstanceId, task);

        // Register TTL for cleanup
        orchestrator.registerEphemeralTTL(instance.id, ttlMinutes);

        // Get the container's chat URL
        const chatUrl = await orchestrator.ensureContainerRunning(instance.id);

        if (asyncMode) {
          // Async: return immediately, client subscribes to result channel
          res.status(202).json({
            instanceId: instance.id,
            status: 'running',
            resultChannel: `ephemeral:${instance.id}:result`,
          });

          // Fire-and-forget: forward task and update status on completion
          (async () => {
            try {
              const chatRes = await fetch(`${chatUrl}/chat`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ message: task, sessionId: instance.id }),
                signal: AbortSignal.timeout(ttlMinutes * 60_000),
              });
              const body = (await chatRes.json()) as {
                result: string | null;
                error?: string;
                usage?: { promptTokens: number; completionTokens: number; totalTokens: number };
              };
              await agentRegistry.updateInstanceStatus(instance.id, 'completed');
              logger.info(
                `Ephemeral agent ${instanceName} completed in ${Date.now() - startTime}ms`
              );

              const intercom = orchestrator.getIntercom();
              if (intercom) {
                await intercom.publish(`ephemeral:${instance.id}:result`, {
                  instanceId: instance.id,
                  status: body.error ? 'error' : 'completed',
                  result: body.result,
                  error: body.error,
                  usage: body.usage,
                  durationMs: Date.now() - startTime,
                });
              }
            } catch (err) {
              logger.error(`Ephemeral agent ${instanceName} failed:`, err);
              await agentRegistry.updateInstanceStatus(instance.id, 'error');
            }
          })();
        } else {
          // Sync: wait for result
          const chatRes = await fetch(`${chatUrl}/chat`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ message: task, sessionId: instance.id }),
            signal: AbortSignal.timeout(ttlMinutes * 60_000),
          });

          if (!chatRes.ok) {
            const text = await chatRes.text().catch(() => '');
            throw new Error(`Ephemeral agent returned ${chatRes.status}: ${text}`);
          }

          const body = (await chatRes.json()) as {
            result: string | null;
            error?: string;
            usage?: { promptTokens: number; completionTokens: number; totalTokens: number };
          };

          await agentRegistry.updateInstanceStatus(instance.id, 'completed');
          const durationMs = Date.now() - startTime;
          logger.info(`Ephemeral agent ${instanceName} completed in ${durationMs}ms`);

          res.json({
            instanceId: instance.id,
            status: body.error ? 'error' : 'completed',
            result: body.result,
            error: body.error,
            usage: body.usage,
            durationMs,
          });
        }
      } catch (err) {
        await agentRegistry.updateInstanceStatus(instance.id, 'error');
        const msg = err instanceof Error ? err.message : String(err);
        logger.error(`Ephemeral spawn failed for ${instanceName}:`, msg);
        res.status(500).json({
          instanceId: instance.id,
          status: 'error',
          error: msg,
          durationMs: Date.now() - startTime,
        });
      }
    })
  );

  // ── Get agent instance detail ──────────────────────────────────────────────
  router.get(
    '/instances/:id',
    asyncHandler(async (req, res) => {
      const instance = await agentRegistry.getInstance(req.params.id as string);
      if (!instance) {
        res.status(404).json({ error: `Agent instance "${req.params.id as string}" not found` });
        return;
      }
      // Story 3.11: include lineage depth for subagent tree visibility
      const lineageDepth = instance.parent_instance_id
        ? await agentRegistry.getLineageDepth(instance.id)
        : 0;
      // Story 3.12: surface workspace quota fields at top level
      const caps = instance.resolved_capabilities as
        | Record<string, Record<string, unknown>>
        | undefined;
      const workspaceLimitGB = caps?.filesystem?.maxWorkspaceSizeGB as number | undefined;
      res.json({
        ...instance,
        lineageDepth,
        workspaceUsageGB: instance.workspace_used_gb ?? null,
        workspaceLimitGB: workspaceLimitGB ?? null,
      });
    })
  );

  // ── Start an agent instance ────────────────────────────────────────────────
  router.post(
    '/instances/:id/start',
    asyncHandler(async (req, res) => {
      const id = req.params.id as string;
      const instance = await agentRegistry.getInstance(id);
      if (!instance) {
        res.status(404).json({ error: `Agent instance "${id}" not found` });
        return;
      }

      await orchestrator.startInstance(id);
      const updated = await agentRegistry.getInstance(id);
      res.json(updated);
    })
  );

  // ── Stop an agent instance ─────────────────────────────────────────────────
  router.post(
    '/instances/:id/stop',
    asyncHandler(async (req, res) => {
      const id = req.params.id as string;
      const instance = await agentRegistry.getInstance(id);
      if (!instance) {
        res.status(404).json({ error: `Agent instance "${id}" not found` });
        return;
      }

      await orchestrator.stopInstance(id);
      const updated = await agentRegistry.getInstance(id);
      res.json(updated);
    })
  );

  // ── Update an agent instance (overrides, name, display_name, etc.) ───────
  router.patch(
    '/instances/:id',
    asyncHandler(async (req, res) => {
      const id = req.params.id as string;
      const instance = await agentRegistry.getInstance(id);
      if (!instance) {
        res.status(404).json({ error: `Agent instance "${id}" not found` });
        return;
      }

      const { name, displayName, circle, lifecycleMode, overrides } = req.body as {
        name?: string;
        displayName?: string;
        circle?: string;
        lifecycleMode?: string;
        overrides?: Record<string, unknown>;
      };

      await agentRegistry.updateInstance(id, {
        ...(name !== undefined ? { name } : {}),
        ...(displayName !== undefined ? { display_name: displayName } : {}),
        ...(circle !== undefined ? { circle } : {}),
        ...(lifecycleMode !== undefined ? { lifecycle_mode: lifecycleMode } : {}),
        ...(overrides !== undefined ? { overrides } : {}),
      });

      const updated = await agentRegistry.getInstance(id);
      res.json(updated);
    })
  );

  // ── Delete an agent instance ──────────────────────────────────────────────
  router.delete(
    '/instances/:id',
    asyncHandler(async (req, res) => {
      const id = req.params.id as string;
      if (!id) {
        res.status(400).json({ error: 'Instance ID is required' });
        return;
      }

      // Validate UUID format
      const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
      if (!UUID_RE.test(id)) {
        res.status(400).json({ error: 'Invalid instance ID format' });
        return;
      }

      // Verify instance exists before deletion
      const instance = await agentRegistry.getInstance(id);
      if (!instance) {
        res.status(404).json({ error: 'Agent instance not found' });
        return;
      }

      // Stop the instance (cleans up Docker container)
      await orchestrator.stopInstance(id);

      // Delete from DB
      await agentRegistry.deleteInstance(id);

      res.json({
        deleted: { id: instance.id, name: instance.name, circle_id: instance.circle_id },
      });
    })
  );

  // ── Get agent thoughts ───────────────────────────────────────────────────
  router.get(
    '/instances/:id/thoughts',
    asyncHandler(async (req, res) => {
      const id = req.params.id as string;
      const { taskId, limit, offset } = req.query;

      const intercom = orchestrator.getIntercom();
      if (!intercom) {
        res.status(503).json({ error: 'Intercom service not available' });
        return;
      }

      const thoughts = await intercom.getThoughts(id, {
        taskId: taskId as string,
        limit: limit ? parseInt(limit as string) : 50,
        offset: offset ? parseInt(offset as string) : 0,
      });

      res.json(thoughts);
    })
  );

  // ── Legacy: get thoughts by name (redirects to instance ID lookup) ────────
  router.get(
    '/:id/thoughts',
    asyncHandler(async (req, res) => {
      const id = req.params.id as string;
      const { taskId, limit, offset } = req.query;

      const intercom = orchestrator.getIntercom();
      if (!intercom) {
        res.status(503).json({ error: 'Intercom service not available' });
        return;
      }

      const thoughts = await intercom.getThoughts(id, {
        taskId: taskId as string,
        limit: limit ? parseInt(limit as string) : 50,
        offset: offset ? parseInt(offset as string) : 0,
      });

      res.json(thoughts);
    })
  );

  // ── Send a direct message to an agent instance (Story 9.3) ────────────────
  router.post(
    '/:id/message',
    asyncHandler(async (req, res) => {
      try {
        const id = req.params.id as string;
        const { from, payload } = req.body as { from?: string; payload?: Record<string, unknown> };

        if (!from || !payload) {
          res.status(400).json({ error: 'Required fields: from, payload' });
          return;
        }

        const intercom = orchestrator.getIntercom();
        if (!intercom) {
          res.status(503).json({ error: 'Intercom service not available' });
          return;
        }

        let fromManifest = orchestrator.getManifest(from);
        if (!fromManifest) {
          fromManifest = orchestrator.getManifestByInstanceId(from);
        }

        if (!fromManifest) {
          res.status(404).json({ error: `Sender agent "${from}" not found` });
          return;
        }

        const msg = await intercom.sendDirectMessage(fromManifest, id, payload);
        res.json({ success: true, message: msg });
      } catch (err: unknown) {
        const error = err as Error;
        if (error.name === 'IntercomPermissionError') {
          res.status(403).json({ error: error.message });
          return;
        }
        throw err;
      }
    })
  );

  // ── POST /api/agents/validate ────────────────────────────────────────────
  router.post('/validate', (req, res) => {
    const body = req.body;
    if (!body || typeof body !== 'object') {
      res.json({ valid: false, errors: ['Request body must be a JSON object'] });
      return;
    }
    try {
      AgentManifestLoader.validateManifest(body, 'POST /api/agents/validate');
      res.json({ valid: true });
    } catch (err: unknown) {
      const error = err as Error;
      res.json({ valid: false, errors: [error.message] });
    }
  });

  // ── POST /api/agents/test-chat ───────────────────────────────────────────
  router.post(
    '/test-chat',
    asyncHandler(async (req, res) => {
      const { manifest, message, history = [] } = req.body;

      if (!manifest || !message) {
        res.status(400).json({ error: 'manifest and message are required' });
        return;
      }

      AgentManifestLoader.validateManifest(manifest, 'POST /api/agents/test-chat');
      const agent = AgentFactory.createAgent(manifest);

      const toolExecutor = orchestrator.getToolExecutor();
      if (toolExecutor) {
        agent.setToolExecutor(toolExecutor);
      }

      const response = await agent.process(message, history);

      res.json({
        reply: response.finalAnswer || response.thought || 'No response.',
        thought: response.thought,
      });
    })
  );

  // ── System prompt preview ────────────────────────────────────────────────────

  router.get(
    '/:id/system-prompt',
    asyncHandler(async (req, res) => {
      const instanceId = req.params.id as string;
      const instance = await agentRegistry.getInstance(instanceId);
      if (!instance) {
        res.status(404).json({ error: 'Agent instance not found' });
        return;
      }

      const templateRow = await agentRegistry.getTemplate(instance.template_ref);
      if (!templateRow) {
        res.status(404).json({ error: 'Agent template not found' });
        return;
      }

      // Build a minimal manifest from the template spec (mirrors Orchestrator.startInstance)
      const spec = templateRow.spec ?? {};
      const manifest = {
        apiVersion: 'sera/v1' as const,
        kind: 'Agent' as const,
        metadata: {
          name: templateRow.name,
          displayName: templateRow.display_name ?? templateRow.name,
          icon: spec.identity?.icon ?? '',
          circle: spec.circle ?? instance.circle ?? '',
          tier: (spec.sandboxBoundary === 'tier-3'
            ? 3
            : spec.sandboxBoundary === 'tier-2'
              ? 2
              : 1) as 1 | 2 | 3,
        },
        identity: {
          role: spec.identity?.role ?? templateRow.name,
          description: spec.identity?.description ?? '',
          communicationStyle: spec.identity?.communicationStyle,
          principles: spec.identity?.principles,
        },
        model: {
          provider: spec.model?.provider ?? 'default',
          name: spec.model?.name ?? 'default',
        },
        spec,
      };
      const prompt = IdentityService.generateSystemPrompt(manifest);

      res.json({ prompt });
    })
  );

  // ── Health Check ─────────────────────────────────────────────────────────
  /**
   * GET /api/agents/:id/health-check
   * Runs a quick diagnostic for a specific agent instance.
   */
  router.get(
    '/:id/health-check',
    asyncHandler(async (req, res) => {
      const instanceId = String(req.params['id']);

      const checks: Record<string, { ok: boolean; detail?: string }> = {};

      // 1. Instance exists in DB
      const instance = await agentRegistry.getInstance(instanceId);
      checks['instanceExists'] = instance
        ? { ok: true, detail: instance.name }
        : { ok: false, detail: 'Not found in database' };

      if (!instance) {
        res.json({ agentId: instanceId, overallStatus: 'not-found', checks });
        return;
      }

      // 2. Agent loaded in orchestrator
      const liveAgents = orchestrator.listAgents();
      const liveAgent = liveAgents.find((a) => a.id === instanceId);
      checks['orchestratorLoaded'] = liveAgent
        ? { ok: true, detail: liveAgent.status }
        : { ok: false, detail: 'Not loaded in orchestrator' };

      // 3. Manifest available (try by name, then by instance ID for API-created agents)
      const manifest =
        (instance ? orchestrator.getManifest(instance.name) : undefined) ??
        orchestrator.getManifestByInstanceId(instanceId);
      checks['manifestLoaded'] = manifest
        ? { ok: true }
        : { ok: false, detail: 'Manifest not found' };

      // 4. Container running (check via Docker label)
      try {
        const sandboxManager = (orchestrator as unknown as Record<string, unknown>)[
          'sandboxManager'
        ];
        if (
          sandboxManager &&
          typeof (sandboxManager as Record<string, unknown>)['ping'] === 'function'
        ) {
          await (sandboxManager as { ping: () => Promise<void> }).ping();
          checks['dockerReachable'] = { ok: true };
        } else {
          checks['dockerReachable'] = { ok: false, detail: 'SandboxManager not available' };
        }
      } catch (err) {
        checks['dockerReachable'] = {
          ok: false,
          detail: err instanceof Error ? err.message : 'Docker unreachable',
        };
      }

      // 5. Tool executor available
      const toolExecutor = orchestrator.getToolExecutor();
      checks['toolExecutor'] = toolExecutor
        ? { ok: true }
        : { ok: false, detail: 'ToolExecutor not available' };

      // 6. Intercom (Centrifugo) available
      const intercom = orchestrator.getIntercom();
      checks['intercomAvailable'] = intercom
        ? { ok: true }
        : { ok: false, detail: 'IntercomService not available' };

      // Overall status
      const allOk = Object.values(checks).every((c) => c.ok);
      const anyFailed = Object.values(checks).some((c) => !c.ok);
      const overallStatus = allOk ? 'healthy' : anyFailed ? 'degraded' : 'healthy';

      res.json({
        agentId: instanceId,
        agentName: instance.name,
        overallStatus,
        checks,
      });
    })
  );

  // ── Context Debug (#305) ─────────────────────────────────────────────────
  /**
   * GET /api/agents/:id/context-debug?message=...
   * Dry-runs context assembly and returns structured events showing what
   * would be injected into the LLM call (skills, memory, token budget).
   */
  router.get(
    '/:id/context-debug',
    asyncHandler(async (req, res) => {
      const instanceId = String(req.params['id']);
      const testMessage = (req.query['message'] as string) || 'Hello';

      const instance = await agentRegistry.getInstance(instanceId);
      if (!instance) {
        res.status(404).json({ error: 'Agent not found' });
        return;
      }

      // Try live agent manifest first, then orchestrator by name, then build from template
      let manifest: AgentManifest | undefined =
        orchestrator.getManifestByInstanceId(instanceId) ?? orchestrator.getManifest(instance.name);
      if (!manifest && instance.template_ref) {
        const template = await agentRegistry.getTemplate(instance.template_ref);
        if (template) {
          // Build a synthetic manifest from the template + instance overrides
          const overrides = (instance.overrides ?? {}) as Record<string, unknown>;
          const modelOv = (overrides.model as Record<string, unknown>) ?? {};
          const identityOv = (overrides.identity as Record<string, unknown>) ?? {};
          const tplSpec = template.spec ?? {};
          const tplModel = tplSpec.model ?? {};
          const tplIdentity = tplSpec.identity ?? {};
          manifest = {
            apiVersion: 'sera/v1',
            kind: 'Agent',
            metadata: {
              name: instance.name,
              displayName: instance.display_name ?? instance.name,
              icon: tplIdentity.icon ?? '',
              tier: 2 as const,
              ...(instance.circle ? { circle: instance.circle } : {}),
            },
            identity: {
              role: (tplIdentity.role as string) ?? (identityOv.role as string) ?? instance.name,
              description:
                (tplIdentity.description as string) ?? (identityOv.description as string) ?? '',
            },
            model: {
              provider: (modelOv.provider as string) ?? (tplModel.provider as string) ?? 'default',
              name: (modelOv.name as string) ?? (tplModel.name as string) ?? 'default',
              ...(tplModel.temperature !== undefined
                ? { temperature: tplModel.temperature as number }
                : {}),
            },
            spec: tplSpec,
          };
        }
      }
      if (!manifest) {
        res.status(404).json({
          error: 'Agent manifest not found — no template or YAML manifest available',
        });
        return;
      }

      // Build minimal message array for assembly
      const systemPrompt = IdentityService.generateSystemPrompt(manifest);

      const messages = [
        { role: 'system' as const, content: systemPrompt },
        { role: 'user' as const, content: testMessage },
      ];

      const assembler = new ContextAssembler(dbPool, orchestrator);

      const events: ContextAssemblyEvent[] = [];
      try {
        await assembler.assemble(instanceId, messages, (event) => {
          events.push(event);
        });
      } catch (err) {
        events.push({
          stage: 'assembly.error',
          detail: { error: (err as Error).message },
        });
      }

      res.json({
        agentId: instanceId,
        agentName: instance.name,
        testMessage,
        systemPromptLength: systemPrompt.length,
        events,
      });
    })
  );

  return router;
}
