import { v4 as uuidv4 } from 'uuid';
import { query } from '../../lib/database.js';
import type { Skill } from '../SkillRegistry.js';

/**
 * schedule-task Skill
 * Allows agents to create or manage their own recurring tasks.
 */
export const scheduleTaskSkill: Skill = {
  name: 'schedule-task',
  description: 'Schedule a recurring task for the current agent using a cron expression.',
  parameters: {
    type: 'object',
    properties: {
      action: {
        type: 'string',
        enum: ['create', 'list', 'delete', 'update'],
        description: 'The action to perform: create, list, delete, or update a schedule.'
      },
      name: {
        type: 'string',
        description: 'Descriptive name for the schedule (e.g., "Daily Research Summary").'
      },
      cron: {
        type: 'string',
        description: 'Standard cron expression (e.g., "0 9 * * *" for daily at 9am).'
      },
      task: {
        type: 'object',
        description: 'JSON object representing the task to be executed (e.g., { "action": "summarize", "target": "news" }).'
      },
      scheduleId: {
        type: 'string',
        description: 'The UUID of the schedule (required for delete and update).'
      },
      status: {
        type: 'string',
        enum: ['active', 'paused'],
        description: 'New status for the schedule (used with update).'
      }
    },
    required: ['action']
  },
  handler: async (args, context) => {
    const agentId = context.agentId;
    if (!agentId) {
      return { error: 'Skill must be executed in an agent instance context.' };
    }

    const { action, name, cron, task, scheduleId, status } = args;

    try {
      switch (action) {
        case 'create':
          if (!name || !cron || !task) {
            return { error: 'name, cron, and task are required for create action.' };
          }
          const newId = uuidv4();
          const now = new Date().toISOString();
          await query(
            `INSERT INTO schedules (id, agent_id, name, cron, task, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, 'active', $6, $6)`,
            [newId, agentId, name, cron, JSON.stringify(task), now]
          );
          return { success: true, scheduleId: newId, message: `Schedule "${name}" created successfully.` };

        case 'list':
          const listResult = await query(
            'SELECT id, name, cron, task, status, last_run FROM schedules WHERE agent_id = $1',
            [agentId]
          );
          return { schedules: listResult.rows };

        case 'delete':
          if (!scheduleId) return { error: 'scheduleId is required for delete action.' };
          const delRes = await query('DELETE FROM schedules WHERE id = $1 AND agent_id = $2', [scheduleId, agentId]);
          if (delRes.rowCount === 0) return { error: 'Schedule not found or not owned by this agent.' };
          return { success: true, message: 'Schedule deleted successfully.' };

        case 'update':
          if (!scheduleId) return { error: 'scheduleId is required for update action.' };
          const currentRes = await query('SELECT * FROM schedules WHERE id = $1 AND agent_id = $2', [scheduleId, agentId]);
          if (currentRes.rows.length === 0) return { error: 'Schedule not found or not owned by this agent.' };

          const current = currentRes.rows[0];
          const updName = name ?? current.name;
          const updCron = cron ?? current.cron;
          const updTask = task ? JSON.stringify(task) : current.task;
          const updStatus = status ?? current.status;

          await query(
            `UPDATE schedules SET name = $1, cron = $2, task = $3, status = $4, updated_at = NOW()
             WHERE id = $5`,
            [updName, updCron, updTask, updStatus, scheduleId]
          );
          return { success: true, message: 'Schedule updated successfully.' };

        default:
          return { error: `Unsupported action: ${action}` };
      }
    } catch (err: any) {
      return { error: `Database error: ${err.message}` };
    }
  }
};
