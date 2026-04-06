import { query } from '../../lib/database.js';
import { Logger } from '../../lib/logger.js';
import type { SkillDefinition } from '../types.js';

const logger = new Logger('ManageAgent');

/**
 * manage-agent Skill — Issue #sera-uxh
 *
 * Exposes agent lifecycle operations to agents that have seraManagement.agents capability.
 * Actions wrap the SERA core HTTP API rather than calling the Orchestrator directly,
 * keeping the skill stateless and compatible with the agent-runtime sandbox context.
 *
 * Actions:
 *   - start-agent:   start an agent instance (spawns Docker container)
 *   - stop-agent:    stop an agent instance (tears down Docker container)
 *   - restart-agent: stop then start an agent instance
 *   - agent-health:  query health/diagnostics for an agent instance
 *   - list-agents:   list all agent instances with their current status
 */
export const manageAgentSkill: SkillDefinition = {
  id: 'manage-agent',
  description:
    'Manage agent lifecycle. Actions: "start-agent", "stop-agent", "restart-agent", "agent-health", "list-agents".',
  source: 'builtin',
  parameters: [
    {
      name: 'action',
      type: 'string',
      description:
        'Lifecycle action: "start-agent", "stop-agent", "restart-agent", "agent-health", "list-agents".',
      required: true,
    },
    {
      name: 'agentName',
      type: 'string',
      description:
        'Name of the target agent instance (required for all actions except "list-agents").',
      required: false,
    },
    {
      name: 'agentId',
      type: 'string',
      description:
        'UUID of the target agent instance. Used instead of agentName if both are supplied.',
      required: false,
    },
    {
      name: 'statusFilter',
      type: 'string',
      description:
        'For "list-agents": filter by status ("running", "stopped", "error", "idle"). Omit for all.',
      required: false,
    },
  ],
  handler: async (params, _agentContext) => {
    const { action, agentName, agentId, statusFilter } = params as {
      action: string;
      agentName?: string;
      agentId?: string;
      statusFilter?: string;
    };

    const coreUrl = process.env.SERA_CORE_URL ?? 'http://sera-core:3001';
    const apiKey = process.env.SERA_BOOTSTRAP_API_KEY ?? '';

    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${apiKey}`,
    };

    /**
     * Resolve an agent instance ID from name or explicit ID.
     * Returns { id, name } or { error }.
     */
    async function resolveInstance(
      nameOrId: string | undefined,
      explicitId: string | undefined
    ): Promise<{ id: string; name: string } | { error: string }> {
      if (explicitId) {
        const rows = await query('SELECT id, name FROM agent_instances WHERE id = $1', [
          explicitId,
        ]);
        const row = rows.rows[0];
        if (!row) {
          return { error: `Agent instance with id "${explicitId}" not found.` };
        }
        return { id: row.id as string, name: row.name as string };
      }
      if (nameOrId) {
        const rows = await query('SELECT id, name FROM agent_instances WHERE name = $1 LIMIT 1', [
          nameOrId,
        ]);
        const row = rows.rows[0];
        if (!row) {
          return { error: `Agent instance "${nameOrId}" not found.` };
        }
        return { id: row.id as string, name: row.name as string };
      }
      return { error: 'agentName or agentId is required for this action.' };
    }

    try {
      switch (action) {
        // ── List agents ──────────────────────────────────────────────────────
        case 'list-agents': {
          const url = statusFilter
            ? `${coreUrl}/api/agents?status=${encodeURIComponent(statusFilter)}`
            : `${coreUrl}/api/agents`;

          const res = await fetch(url, { headers });
          const body = (await res.json()) as unknown;

          if (!res.ok) {
            const errMsg =
              body !== null && typeof body === 'object' && 'error' in body
                ? String((body as Record<string, unknown>)['error'])
                : res.statusText;
            return { success: false, error: `list-agents failed: ${errMsg}` };
          }

          // The agents endpoint returns an array directly
          const agents = Array.isArray(body) ? body : [];
          logger.info(`manage-agent: listed ${agents.length} agent instances`);
          return {
            success: true,
            data: {
              agents: (agents as Array<Record<string, unknown>>).map((a) => ({
                id: a['id'],
                name: a['name'],
                displayName: a['display_name'],
                status: a['status'],
                templateRef: a['template_ref'],
                lifecycleMode: a['lifecycle_mode'],
              })),
            },
          };
        }

        // ── Start agent ──────────────────────────────────────────────────────
        case 'start-agent': {
          const resolved = await resolveInstance(agentName, agentId);
          if ('error' in resolved) {
            return { success: false, error: resolved.error };
          }

          const res = await fetch(`${coreUrl}/api/agents/instances/${resolved.id}/start`, {
            method: 'POST',
            headers,
          });
          const body = (await res.json()) as Record<string, unknown>;

          if (!res.ok) {
            const errMsg = body['error'] ? String(body['error']) : res.statusText;
            return {
              success: false,
              error: `start-agent failed for "${resolved.name}": ${errMsg}`,
            };
          }

          logger.info(`manage-agent: started instance ${resolved.id} (${resolved.name})`);
          return {
            success: true,
            data: {
              agentId: resolved.id,
              agentName: resolved.name,
              status: body['status'],
              message: `Agent "${resolved.name}" started successfully.`,
            },
          };
        }

        // ── Stop agent ───────────────────────────────────────────────────────
        case 'stop-agent': {
          const resolved = await resolveInstance(agentName, agentId);
          if ('error' in resolved) {
            return { success: false, error: resolved.error };
          }

          const res = await fetch(`${coreUrl}/api/agents/instances/${resolved.id}/stop`, {
            method: 'POST',
            headers,
          });
          const body = (await res.json()) as Record<string, unknown>;

          if (!res.ok) {
            const errMsg = body['error'] ? String(body['error']) : res.statusText;
            return { success: false, error: `stop-agent failed for "${resolved.name}": ${errMsg}` };
          }

          logger.info(`manage-agent: stopped instance ${resolved.id} (${resolved.name})`);
          return {
            success: true,
            data: {
              agentId: resolved.id,
              agentName: resolved.name,
              status: body['status'],
              message: `Agent "${resolved.name}" stopped successfully.`,
            },
          };
        }

        // ── Restart agent ────────────────────────────────────────────────────
        case 'restart-agent': {
          const resolved = await resolveInstance(agentName, agentId);
          if ('error' in resolved) {
            return { success: false, error: resolved.error };
          }

          const res = await fetch(`${coreUrl}/api/agents/${resolved.id}/restart`, {
            method: 'POST',
            headers,
          });
          const body = (await res.json()) as Record<string, unknown>;

          if (!res.ok) {
            const errMsg = body['error'] ? String(body['error']) : res.statusText;
            return {
              success: false,
              error: `restart-agent failed for "${resolved.name}": ${errMsg}`,
            };
          }

          logger.info(`manage-agent: restarted instance ${resolved.id} (${resolved.name})`);
          return {
            success: true,
            data: {
              agentId: resolved.id,
              agentName: resolved.name,
              message: `Agent "${resolved.name}" restarted successfully.`,
            },
          };
        }

        // ── Agent health ─────────────────────────────────────────────────────
        case 'agent-health': {
          const resolved = await resolveInstance(agentName, agentId);
          if ('error' in resolved) {
            return { success: false, error: resolved.error };
          }

          const res = await fetch(`${coreUrl}/api/agents/${resolved.id}/health-check`, {
            headers,
          });
          const body = (await res.json()) as Record<string, unknown>;

          if (!res.ok) {
            const errMsg = body['error'] ? String(body['error']) : res.statusText;
            return {
              success: false,
              error: `agent-health failed for "${resolved.name}": ${errMsg}`,
            };
          }

          logger.info(`manage-agent: health check for instance ${resolved.id} (${resolved.name})`);
          return {
            success: true,
            data: {
              agentId: resolved.id,
              agentName: resolved.name,
              status: body['status'],
              checks: body['checks'],
            },
          };
        }

        default:
          return {
            success: false,
            error: `Unknown action "${action}". Valid actions: start-agent, stop-agent, restart-agent, agent-health, list-agents.`,
          };
      }
    } catch (err: unknown) {
      logger.error('manage-agent error:', err);
      return { success: false, error: `manage-agent error: ${(err as Error).message}` };
    }
  },
};
