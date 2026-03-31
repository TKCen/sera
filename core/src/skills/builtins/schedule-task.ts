import { v4 as uuidv4 } from 'uuid';
import { query } from '../../lib/database.js';
import type { SkillDefinition } from '../types.js';

/**
 * schedule-task Skill
 * Allows agents to create or manage their own recurring tasks.
 */
export const scheduleTaskSkill: SkillDefinition = {
  id: 'schedule-task',
  description: 'Schedule a recurring task for the current agent using a cron expression.',
  source: 'builtin',
  parameters: [
    {
      name: 'action',
      type: 'string',
      description: 'The action to perform: create, list, delete, or update a schedule.',
      required: true,
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
      description: 'The UUID of the schedule (required for delete and update).',
      required: false,
    },
    {
      name: 'status',
      type: 'string',
      description: 'New status for the schedule (used with update).',
      required: false,
    },
  ],
  handler: async (params, context) => {
    const agentId = context.agentInstanceId;
    if (!agentId) {
      return { success: false, error: 'Skill must be executed in an agent instance context.' };
    }

    const { action, name, cron, task, scheduleId, status } = params as {
      action: string;
      name?: string;
      cron?: string;
      task?: string | object;
      scheduleId?: string;
      status?: string;
    };

    // Normalize task to a string prompt — the LLM may send a string or an object
    const taskPrompt =
      task == null ? undefined : typeof task === 'string' ? task : JSON.stringify(task);

    try {
      switch (action) {
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
            `INSERT INTO schedules (id, agent_instance_id, agent_name, name, expression, type, task, source, status, created_at, updated_at)
             VALUES ($1, $2, (SELECT name FROM agent_instances WHERE id = $2), $3, $4, 'cron', $5, 'api', 'active', $6, $6)`,
            [newId, agentId, name, cron, taskPrompt, now]
          );
          return {
            success: true,
            data: { scheduleId: newId, message: `Schedule "${name}" created successfully.` },
          };
        }

        case 'list': {
          const listResult = await query(
            `SELECT id, name, expression AS cron, task, status, last_run_at AS last_run
             FROM schedules WHERE agent_instance_id = $1`,
            [agentId]
          );
          return { success: true, data: { schedules: listResult.rows } };
        }

        case 'delete': {
          if (!scheduleId)
            return { success: false, error: 'scheduleId is required for delete action.' };
          const delRes = await query(
            'DELETE FROM schedules WHERE id = $1 AND agent_instance_id = $2',
            [scheduleId, agentId]
          );
          if (delRes.rowCount === 0)
            return { success: false, error: 'Schedule not found or not owned by this agent.' };
          return { success: true, data: { message: 'Schedule deleted successfully.' } };
        }

        case 'update': {
          if (!scheduleId)
            return { success: false, error: 'scheduleId is required for update action.' };
          const currentRes = await query(
            'SELECT * FROM schedules WHERE id = $1 AND agent_instance_id = $2',
            [scheduleId, agentId]
          );
          if (currentRes.rows.length === 0)
            return { success: false, error: 'Schedule not found or not owned by this agent.' };

          const current = currentRes.rows[0];
          const updName = name ?? current.name;
          const updCron = cron ?? (current.expression || current.cron);
          const updTask = taskPrompt ?? current.task;
          const updStatus = status ?? current.status;

          await query(
            `UPDATE schedules SET name = $1, expression = $2, task = $3, status = $4, updated_at = NOW()
             WHERE id = $5`,
            [updName, updCron, updTask, updStatus, scheduleId]
          );
          return { success: true, data: { message: 'Schedule updated successfully.' } };
        }

        default:
          return { success: false, error: `Unsupported action: ${action}` };
      }
    } catch (err: unknown) {
      return { success: false, error: `Database error: ${(err as Error).message}` };
    }
  },
};
