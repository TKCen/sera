import { v4 as uuidv4 } from 'uuid';
import { query } from '../../lib/database.js';
import type { AgentContext, SkillDefinition } from '../types.js';

/**
 * schedule-task Skill
 * Allows agents to create or manage recurring tasks for themselves or other agents.
 *
 * Permission scopes (checked via seraManagement.schedules capabilities):
 * - personal: manage own schedules only (default)
 * - circle:   manage schedules for agents in the same circle
 * - global:   manage schedules for any agent
 */

type ScheduleScope = 'personal' | 'circle' | 'global';

/**
 * Resolve the permission scope needed for an operation and verify the
 * calling agent has the required capability.
 *
 * Returns the resolved target agent instance ID, or an error message.
 */
async function resolveTargetAgent(
  callerAgentId: string,
  targetAgentId: string | undefined,
  context: AgentContext
): Promise<{ targetId: string; scope: ScheduleScope } | { error: string }> {
  // No target specified or targeting self — personal scope, always allowed
  if (!targetAgentId || targetAgentId === callerAgentId) {
    return { targetId: callerAgentId, scope: 'personal' };
  }

  // Targeting another agent — determine required scope
  const callerCircle = context.manifest?.metadata?.circle ?? null;

  // Look up target agent's circle
  const targetRes = await query('SELECT id, circle FROM agent_instances WHERE id = $1', [
    targetAgentId,
  ]);
  if (targetRes.rows.length === 0) {
    return { error: `Target agent ${targetAgentId} not found.` };
  }
  const targetCircle = targetRes.rows[0].circle as string | null;

  // Determine scope: circle if both share a circle, otherwise global
  const sameCircle = callerCircle && targetCircle && callerCircle === targetCircle;
  const requiredScope: ScheduleScope = sameCircle ? 'circle' : 'global';

  // Check capability
  const caps = context.manifest?.spec?.capabilities as Record<string, unknown> | undefined;
  const schedCaps = (caps?.seraManagement as Record<string, unknown>)?.schedules as
    | Record<string, unknown>
    | undefined;

  const hasPermission = checkScopePermission(schedCaps, 'create', requiredScope);
  if (!hasPermission) {
    return {
      error: `Insufficient permission: managing schedules for ${requiredScope}-scope agents requires schedules.create.allow to include "${requiredScope === 'circle' ? 'own-circle' : 'global'}".`,
    };
  }

  return { targetId: targetAgentId, scope: requiredScope };
}

/**
 * Check if a capability section grants the required scope for an operation.
 *
 * Capability format in template YAML:
 *   schedules:
 *     create:
 *       allow: [own-circle]   →  permits personal + circle
 *     read: true              →  permits all scopes for read
 *
 * Scope hierarchy: global > circle > personal
 */
function checkScopePermission(
  schedCaps: Record<string, unknown> | undefined,
  operation: string,
  requiredScope: ScheduleScope
): boolean {
  if (!schedCaps) return false;

  const opConfig = schedCaps[operation];

  // Boolean true = unrestricted for this operation
  if (opConfig === true) return true;

  // Object with allow array
  if (typeof opConfig === 'object' && opConfig !== null && 'allow' in opConfig) {
    const allowList = (opConfig as { allow: string[] }).allow;
    if (!Array.isArray(allowList)) return false;

    // Scope mapping: what allow values grant what scopes
    if (requiredScope === 'personal') {
      // Always allowed if any schedule permission exists
      return true;
    }
    if (requiredScope === 'circle') {
      return allowList.includes('own-circle') || allowList.includes('global');
    }
    if (requiredScope === 'global') {
      return allowList.includes('global');
    }
  }

  return false;
}

export const scheduleTaskSkill: SkillDefinition = {
  id: 'schedule-task',
  description:
    'Schedule a recurring task, or manage existing schedules (list, get, activate, deactivate, update, delete) for the current agent or another agent.',
  source: 'builtin',
  parameters: [
    {
      name: 'action',
      type: 'string',
      description:
        'The action to perform: create, list, get, activate, deactivate, update, or delete a schedule.',
      required: true,
    },
    {
      name: 'targetAgentId',
      type: 'string',
      description:
        'UUID of the agent to manage schedules for. Omit to manage own schedules. Requires circle or global permission to target other agents.',
      required: false,
    },
    {
      name: 'name',
      type: 'string',
      description: 'Descriptive name for the schedule (e.g., "Daily Research Summary").',
      required: false,
    },
    {
      name: 'cron',
      type: 'string',
      description: 'Standard cron expression (e.g., "0 9 * * *" for daily at 9am).',
      required: false,
    },
    {
      name: 'task',
      type: 'string',
      description: 'The prompt/instruction to execute when the schedule fires.',
      required: false,
    },
    {
      name: 'scheduleId',
      type: 'string',
      description:
        'The UUID of the schedule (required for get, activate, deactivate, delete, and update).',
      required: false,
    },
    {
      name: 'status',
      type: 'string',
      description: 'New status for the schedule (used with update).',
      required: false,
    },
    {
      name: 'category',
      type: 'string',
      description:
        'Category for the schedule (e.g., "reflection", "knowledge_consolidation", "curiosity_research", "goal_review", "schedule_review", "general").',
      required: false,
    },
  ],
  handler: async (params, context) => {
    const callerAgentId = context.agentInstanceId;
    if (!callerAgentId) {
      return { success: false, error: 'Skill must be executed in an agent instance context.' };
    }

    const { action, targetAgentId, name, cron, task, scheduleId, status, category } = params as {
      action: string;
      targetAgentId?: string;
      name?: string;
      cron?: string;
      task?: string | object;
      scheduleId?: string;
      status?: string;
      category?: string;
    };

    // Resolve target agent and check permissions
    const resolved = await resolveTargetAgent(callerAgentId, targetAgentId, context);
    if ('error' in resolved) {
      return { success: false, error: resolved.error };
    }
    const { targetId } = resolved;

    // Normalize action aliases — LLMs may use synonyms for the canonical action names.
    const ACTION_ALIASES: Record<string, string> = {
      add: 'create',
      new: 'create',
      schedule: 'create',
      remove: 'delete',
      destroy: 'delete',
      cancel: 'delete',
      pause: 'deactivate',
      disable: 'deactivate',
      stop: 'deactivate',
      resume: 'activate',
      enable: 'activate',
      start: 'activate',
      edit: 'update',
      modify: 'update',
      patch: 'update',
      show: 'get',
      fetch: 'get',
      read: 'get',
      'list-schedules': 'list',
      'get-all': 'list',
    };
    const normalizedAction = ACTION_ALIASES[action] ?? action;

    // Normalize task to a JSON string — the `task` column is type JSON in Postgres.
    // The LLM may send a plain string prompt or a structured object.
    const taskPrompt =
      task == null
        ? undefined
        : typeof task === 'string'
          ? JSON.stringify({ prompt: task })
          : JSON.stringify(task);

    try {
      switch (normalizedAction) {
        case 'create': {
          if (!name || !cron || !taskPrompt) {
            return {
              success: false,
              error: 'name, cron, and task are required for create action.',
            };
          }
          const newId = uuidv4();
          const now = new Date().toISOString();
          await query(
            `INSERT INTO schedules (id, agent_instance_id, agent_name, name, expression, type, task, source, status, category, created_at, updated_at)
             VALUES ($1, $2, (SELECT name FROM agent_instances WHERE id = $2), $3, $4, 'cron', $5, 'api', 'active', $6, $7, $7)`,
            [newId, targetId, name, cron, taskPrompt, category ?? null, now]
          );
          const targetLabel = targetId === callerAgentId ? '' : ` for agent ${targetId}`;
          return {
            success: true,
            data: {
              scheduleId: newId,
              message: `Schedule "${name}" created successfully${targetLabel}.`,
            },
          };
        }

        case 'list': {
          const listResult = await query(
            `SELECT id, name, expression AS cron, task, status, category, source, description, last_run_at AS last_run, next_run_at
             FROM schedules WHERE agent_instance_id = $1`,
            [targetId]
          );
          return { success: true, data: { schedules: listResult.rows } };
        }

        case 'get': {
          if (!scheduleId)
            return { success: false, error: 'scheduleId is required for get action.' };
          const getRes = await query(
            `SELECT id, name, expression AS cron, task, status, category, source, description, last_run_at, next_run_at, created_at, updated_at
             FROM schedules WHERE id = $1 AND agent_instance_id = $2`,
            [scheduleId, targetId]
          );
          if (getRes.rows.length === 0)
            return {
              success: false,
              error: 'Schedule not found or not owned by the target agent.',
            };
          return { success: true, data: { schedule: getRes.rows[0] } };
        }

        case 'activate': {
          if (!scheduleId)
            return { success: false, error: 'scheduleId is required for activate action.' };
          const actRes = await query(
            `UPDATE schedules SET status = 'active', updated_at = NOW()
             WHERE id = $1 AND agent_instance_id = $2`,
            [scheduleId, targetId]
          );
          if (actRes.rowCount === 0)
            return {
              success: false,
              error: 'Schedule not found or not owned by the target agent.',
            };
          return { success: true, data: { message: 'Schedule activated successfully.' } };
        }

        case 'deactivate': {
          if (!scheduleId)
            return { success: false, error: 'scheduleId is required for deactivate action.' };
          const deactRes = await query(
            `UPDATE schedules SET status = 'paused', updated_at = NOW()
             WHERE id = $1 AND agent_instance_id = $2`,
            [scheduleId, targetId]
          );
          if (deactRes.rowCount === 0)
            return {
              success: false,
              error: 'Schedule not found or not owned by the target agent.',
            };
          return {
            success: true,
            data: { message: 'Schedule deactivated (paused) successfully.' },
          };
        }

        case 'delete': {
          if (!scheduleId)
            return { success: false, error: 'scheduleId is required for delete action.' };
          // Check if manifest-sourced — cannot delete
          const sourceCheck = await query(
            'SELECT source FROM schedules WHERE id = $1 AND agent_instance_id = $2',
            [scheduleId, targetId]
          );
          if (sourceCheck.rows.length === 0)
            return {
              success: false,
              error: 'Schedule not found or not owned by the target agent.',
            };
          if (sourceCheck.rows[0]!.source === 'manifest')
            return {
              success: false,
              error: 'Cannot delete manifest-managed schedules. Use deactivate instead.',
            };
          const delRes = await query(
            'DELETE FROM schedules WHERE id = $1 AND agent_instance_id = $2',
            [scheduleId, targetId]
          );
          if (delRes.rowCount === 0)
            return {
              success: false,
              error: 'Schedule not found or not owned by the target agent.',
            };
          return { success: true, data: { message: 'Schedule deleted successfully.' } };
        }

        case 'update': {
          if (!scheduleId)
            return { success: false, error: 'scheduleId is required for update action.' };
          const currentRes = await query(
            'SELECT * FROM schedules WHERE id = $1 AND agent_instance_id = $2',
            [scheduleId, targetId]
          );
          if (currentRes.rows.length === 0)
            return {
              success: false,
              error: 'Schedule not found or not owned by the target agent.',
            };

          const current = currentRes.rows[0];

          // Manifest schedules: only allow status changes (activate/deactivate)
          if (current.source === 'manifest' && (name || cron || taskPrompt || category)) {
            return {
              success: false,
              error:
                'Cannot modify expression, task, or name of manifest-managed schedules. Use activate/deactivate to change status.',
            };
          }

          const updName = name ?? current.name;
          const updCron = cron ?? (current.expression || current.cron);
          const updTask = taskPrompt ?? current.task;
          const updStatus = status ?? current.status;
          const updCategory = category ?? current.category ?? null;

          await query(
            `UPDATE schedules SET name = $1, expression = $2, task = $3, status = $4, category = $5, updated_at = NOW()
             WHERE id = $6`,
            [updName, updCron, updTask, updStatus, updCategory, scheduleId]
          );
          return { success: true, data: { message: 'Schedule updated successfully.' } };
        }

        default:
          return {
            success: false,
            error: `Unsupported action: "${action}". Valid actions: create, list, get, activate, deactivate, update, delete.`,
          };
      }
    } catch (err: unknown) {
      return { success: false, error: `Database error: ${(err as Error).message}` };
    }
  },
};
