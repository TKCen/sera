import { v4 as uuidv4 } from 'uuid';
import { query, pool } from '../../lib/database.js';
import { Logger } from '../../lib/logger.js';
import type { SkillDefinition } from '../types.js';

const logger = new Logger('DelegateTask');

/**
 * delegate-task Skill — Epic 17 / Issue #268
 *
 * Allows an agent to delegate a task to another agent via the task queue.
 * The delegating agent can:
 *   - send: enqueue a task for a target agent (fire-and-forget or wait)
 *   - check: poll for the result of a previously delegated task
 *   - list-agents: discover which agents are available for delegation
 *
 * The delegation is recorded in the task_queue table with a
 * `delegated_by` field linking back to the originating agent.
 */
export const delegateTaskSkill: SkillDefinition = {
  id: 'delegate-task',
  description:
    'Delegate a task to another agent. Actions: "send" to delegate, "check" to poll result, "list-agents" to discover, "spawn-ephemeral" to create+execute a temporary agent.',
  source: 'builtin',
  parameters: [
    {
      name: 'action',
      type: 'string',
      description: 'Action: "send" to delegate, "check" to poll result, "list-agents" to discover.',
      required: true,
    },
    {
      name: 'targetAgent',
      type: 'string',
      description: 'Name of the target agent to delegate to (required for "send").',
      required: false,
    },
    {
      name: 'task',
      type: 'string',
      description: 'The task prompt to send to the target agent (required for "send").',
      required: false,
    },
    {
      name: 'context',
      type: 'object',
      description: 'Optional JSON context to pass along with the task.',
      required: false,
    },
    {
      name: 'priority',
      type: 'number',
      description: 'Task priority (lower = higher priority, default 100).',
      required: false,
    },
    {
      name: 'taskId',
      type: 'string',
      description: 'Task ID to check status/result for (required for "check").',
      required: false,
    },
    {
      name: 'waitForResult',
      type: 'boolean',
      description:
        'If true, wait for the delegated task to complete and return the result inline (max 2 min). Default: false (fire-and-forget).',
      required: false,
    },
  ],
  handler: async (params, agentContext) => {
    const callerInstanceId = agentContext.agentInstanceId;
    const callerName = agentContext.agentName;

    if (!callerInstanceId) {
      return { success: false, error: 'delegate-task must run in an agent instance context.' };
    }

    const { action, targetAgent, task, context, priority, taskId, waitForResult } = params as {
      action: string;
      targetAgent?: string;
      task?: string;
      context?: unknown;
      priority?: number;
      taskId?: string;
      waitForResult?: boolean;
    };

    try {
      switch (action) {
        // ── List available agents for delegation ──────────────────────────
        case 'list-agents': {
          const result = await query(
            `SELECT id, name, display_name, status, template_ref
             FROM agent_instances
             WHERE id != $1 AND status IN ('running', 'idle')
             ORDER BY name`,
            [callerInstanceId]
          );
          return {
            success: true,
            data: {
              agents: result.rows.map((r) => ({
                id: r.id,
                name: r.name,
                displayName: r.display_name,
                status: r.status,
                template: r.template_ref,
              })),
            },
          };
        }

        // ── Send/delegate a task to another agent ────────────────────────
        case 'send': {
          if (!targetAgent) {
            return { success: false, error: 'targetAgent is required for "send" action.' };
          }
          if (!task) {
            return { success: false, error: 'task is required for "send" action.' };
          }

          // Resolve target agent instance
          const targetRows = await query(
            `SELECT id, name, lifecycle_mode FROM agent_instances
             WHERE name = $1 AND status IN ('running', 'idle')
             LIMIT 1`,
            [targetAgent]
          );

          if (targetRows.rows.length === 0) {
            return {
              success: false,
              error: `Agent "${targetAgent}" not found or not running.`,
            };
          }

          const target = targetRows.rows[0]!;

          if (target.lifecycle_mode === 'ephemeral') {
            return {
              success: false,
              error: 'Cannot delegate to ephemeral agents via task queue.',
            };
          }

          // Enqueue the task
          const newTaskId = uuidv4();
          const resolvedPriority = priority ?? 100;

          const delegationContext = {
            ...(context && typeof context === 'object' ? context : {}),
            delegation: {
              fromAgent: callerName,
              fromInstanceId: callerInstanceId,
              delegatedAt: new Date().toISOString(),
            },
          };

          await pool.query(
            `INSERT INTO task_queue
               (id, agent_instance_id, task, context, priority, max_retries, status)
             VALUES ($1, $2, $3, $4, $5, 3, 'queued')`,
            [newTaskId, target.id, task, JSON.stringify(delegationContext), resolvedPriority]
          );

          logger.info(
            `Agent ${callerName} delegated task ${newTaskId} to ${targetAgent} (${target.id})`
          );

          // If waitForResult is set, poll until the task completes or times out
          if (waitForResult) {
            const maxWaitMs = 120_000;
            const pollIntervalMs = 3_000;
            const deadline = Date.now() + maxWaitMs;

            while (Date.now() < deadline) {
              await new Promise((r) => setTimeout(r, pollIntervalMs));
              const { rows } = await pool.query(
                'SELECT status, result, error, exit_reason FROM task_queue WHERE id = $1',
                [newTaskId]
              );
              const t = rows[0];
              if (t?.status === 'completed' || t?.status === 'failed') {
                return {
                  success: t.status === 'completed',
                  data: {
                    taskId: newTaskId,
                    targetAgent,
                    status: t.status,
                    result: t.result,
                    error: t.error,
                    exitReason: t.exit_reason,
                  },
                };
              }
            }

            // Timed out — return what we know
            const { rows: finalRows } = await pool.query(
              'SELECT status FROM task_queue WHERE id = $1',
              [newTaskId]
            );
            const finalStatus = finalRows[0]?.status ?? 'unknown';
            return {
              success: false,
              error: `Delegation to ${targetAgent} timed out after ${maxWaitMs / 1000}s. Task ${newTaskId} is still "${finalStatus}". Use action "check" with taskId "${newTaskId}" to poll later.`,
            };
          }

          return {
            success: true,
            data: {
              taskId: newTaskId,
              targetAgent: targetAgent,
              targetInstanceId: target.id,
              status: 'queued',
              message: `Task delegated to ${targetAgent}. Use action "check" with taskId "${newTaskId}" to poll for results.`,
            },
          };
        }

        // ── Check status/result of a delegated task ──────────────────────
        case 'check': {
          if (!taskId) {
            return { success: false, error: 'taskId is required for "check" action.' };
          }

          const taskRows = await query(
            `SELECT id, agent_instance_id, task, status, result, error,
                    created_at, started_at, completed_at, exit_reason
             FROM task_queue WHERE id = $1`,
            [taskId]
          );

          if (taskRows.rows.length === 0) {
            return { success: false, error: `Task "${taskId}" not found.` };
          }

          const t = taskRows.rows[0]!;

          // Look up agent name for the response
          const agentNameRows = await query('SELECT name FROM agent_instances WHERE id = $1', [
            t.agent_instance_id,
          ]);
          const agentName = agentNameRows.rows[0]?.name ?? t.agent_instance_id;

          return {
            success: true,
            data: {
              taskId: t.id,
              targetAgent: agentName,
              status: t.status,
              result: t.result,
              error: t.error,
              exitReason: t.exit_reason,
              createdAt: t.created_at,
              startedAt: t.started_at,
              completedAt: t.completed_at,
              isComplete: t.status === 'completed' || t.status === 'failed',
            },
          };
        }

        case 'spawn-ephemeral': {
          const templateRef = params.templateRef as string;
          const spawnTask = params.task as string;
          const ttlMinutes = (params.ttlMinutes as number) || 30;

          if (!templateRef || !spawnTask) {
            return {
              success: false,
              error: 'spawn-ephemeral requires "templateRef" and "task" parameters.',
            };
          }

          // Call the spawn-ephemeral API endpoint internally
          const coreUrl = process.env.SERA_CORE_URL ?? 'http://sera-core:3001';
          const apiKey = process.env.SERA_BOOTSTRAP_API_KEY ?? '';
          const spawnRes = await fetch(`${coreUrl}/api/agents/spawn-ephemeral`, {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              Authorization: `Bearer ${apiKey}`,
            },
            body: JSON.stringify({
              templateRef,
              task: spawnTask,
              parentInstanceId: callerInstanceId,
              ttlMinutes,
            }),
            signal: AbortSignal.timeout(ttlMinutes * 60_000),
          });

          const spawnBody = (await spawnRes.json()) as Record<string, unknown>;

          if (!spawnRes.ok) {
            return {
              success: false,
              error: `Ephemeral spawn failed: ${spawnBody.error ?? spawnRes.statusText}`,
            };
          }

          return {
            success: true,
            data: spawnBody,
          };
        }

        default:
          return {
            success: false,
            error: `Unknown action "${action}". Use "send", "check", "list-agents", or "spawn-ephemeral".`,
          };
      }
    } catch (err: unknown) {
      logger.error('delegate-task error:', err);
      return { success: false, error: `Delegation error: ${(err as Error).message}` };
    }
  },
};
