import type { PgBoss } from 'pg-boss';
import { pool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';
import type { Orchestrator } from '../agents/Orchestrator.js';
import { AuditService } from '../audit/AuditService.js';
import { ChannelRouter } from '../channels/ChannelRouter.js';
import { v4 as uuidv4 } from 'uuid';
import { validateCronExpression, computeNextRunAt } from '../lib/cron-utils.js';

const logger = new Logger('ScheduleService');

export interface Schedule {
  id: string;
  agent_instance_id: string;
  agent_name: string;
  name: string;
  description?: string;
  type: 'cron' | 'once';
  expression: string;
  task: string;
  status: 'active' | 'paused' | 'completed' | 'error';
  source: 'manifest' | 'api';
  category?: string;
  last_run_at?: Date;
  next_run_at?: Date;
  last_run_status?: string;
}

export class ScheduleService {
  private static instance: ScheduleService;
  private boss: PgBoss | null = null;
  private orchestrator: Orchestrator | null = null;
  private initialized = false;

  private constructor() {}

  public static getInstance(): ScheduleService {
    if (!ScheduleService.instance) {
      ScheduleService.instance = new ScheduleService();
    }
    return ScheduleService.instance;
  }

  public setOrchestrator(orchestrator: Orchestrator): void {
    this.orchestrator = orchestrator;
  }

  public async start(boss: PgBoss): Promise<void> {
    if (this.initialized) return;

    this.boss = boss;

    // Ensure the queue exists before subscribing a worker (pg-boss v9+ requirement)
    await this.boss.createQueue('agent-schedule');

    // Register worker for agent schedules
    await this.boss.work('agent-schedule', async (jobs) => {
      for (const job of jobs) {
        const { scheduleId } = job.data as { scheduleId: string };
        await this.triggerSchedule(scheduleId).catch((err) => {
          logger.error(`Failed to trigger schedule ${scheduleId}:`, err);
        });
      }
    });

    await this.reconcile();
    this.initialized = true;
    logger.info('ScheduleService started and reconciled');
  }

  /**
   * Reconciles DB schedules with pg-boss jobs.
   */
  public async reconcile(): Promise<void> {
    if (!this.boss) return;

    logger.info('Reconciling schedules...');

    // 1. Get all active cron schedules from DB
    const { rows: dbSchedules } = await pool.query<Schedule>(
      "SELECT * FROM schedules WHERE type = 'cron' AND status = 'active'"
    );

    // 2. Register/Update in pg-boss
    await Promise.all(
      dbSchedules.map(async (schedule) => {
        try {
          await this.ensureQueueAndSchedule(schedule.id, schedule.expression);

          // Compute and store next_run_at
          const nextRunAt = computeNextRunAt(schedule.expression);
          if (nextRunAt) {
            await pool.query('UPDATE schedules SET next_run_at = $1 WHERE id = $2', [
              nextRunAt,
              schedule.id,
            ]);
          }
        } catch (err) {
          logger.error(`Failed to schedule ${schedule.name} (${schedule.id}):`, err);
          await pool.query(
            "UPDATE schedules SET status = 'error', last_run_status = $1 WHERE id = $2",
            [(err as Error).message, schedule.id]
          );
        }
      })
    );

    // 3. Remove stale schedules from pg-boss
    // We can query pgboss.schedule table directly
    const { rows: pgbossSchedules } = await pool.query(
      "SELECT name FROM pgboss.schedule WHERE name NOT IN (SELECT id::text FROM schedules WHERE type = 'cron' AND status = 'active')"
    );

    await Promise.all(
      pgbossSchedules.map(async (row) => {
        // Ignore internal jobs like 'memory-compaction'
        if (uuidValidate(row.name)) {
          logger.info(`Removing stale pg-boss schedule: ${row.name}`);
          await this.boss!.unschedule(row.name);
        }
      })
    );
  }

  public async createSchedule(data: Partial<Schedule>): Promise<Schedule> {
    const id = uuidv4();
    const {
      agent_instance_id,
      agent_name,
      name,
      type,
      expression,
      task,
      source = 'api',
      status: initialStatus = 'active',
      category,
    } = data;

    if (!agent_instance_id || !name || !type || !expression || !task) {
      throw new Error('Missing required fields for schedule');
    }

    // Validate cron expression
    if (type === 'cron') {
      const cronError = validateCronExpression(expression);
      if (cronError) throw new Error(`Invalid cron expression: ${cronError}`);
    }

    // Normalize task to valid JSON for the JSONB column.
    // Plain strings (e.g. from template YAML) are wrapped as { prompt: "..." }.
    const taskJson = normalizeTaskJson(task);

    // Compute next_run_at for active cron schedules
    const nextRunAt =
      type === 'cron' && initialStatus === 'active' ? computeNextRunAt(expression) : null;

    const { rows } = await pool.query<Schedule>(
      `INSERT INTO schedules
       (id, agent_instance_id, agent_name, name, description, type, expression, task, status, source, category, next_run_at)
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
       RETURNING *`,
      [
        id,
        agent_instance_id,
        agent_name,
        name,
        data.description ?? null,
        type,
        expression,
        taskJson,
        initialStatus,
        source,
        category ?? null,
        nextRunAt,
      ]
    );

    const schedule = rows[0]!;

    if (this.boss && schedule.type === 'cron' && schedule.status === 'active') {
      await this.ensureQueueAndSchedule(schedule.id, schedule.expression);
    }

    await AuditService.getInstance().record({
      actorType: 'system',
      actorId: 'system',
      actingContext: null,
      eventType: 'schedule.created',
      payload: {
        scheduleId: schedule.id,
        name: schedule.name,
        agentId: schedule.agent_instance_id,
      },
    });

    return schedule;
  }

  public async updateSchedule(id: string, updates: Partial<Schedule>): Promise<Schedule> {
    const fields = Object.keys(updates).filter((k) =>
      ['name', 'description', 'expression', 'task', 'status', 'category'].includes(k)
    );

    // Validate cron expression if being updated
    if (updates.expression) {
      const cronError = validateCronExpression(updates.expression);
      if (cronError) throw new Error(`Invalid cron expression: ${cronError}`);
    }

    if (fields.length === 0) {
      const { rows } = await pool.query<Schedule>('SELECT * FROM schedules WHERE id = $1', [id]);
      return rows[0]!;
    }

    const setClause = fields.map((f, i) => `${f} = $${i + 2}`).join(', ');
    const values = fields.map((f) => (updates as Record<string, unknown>)[f]);

    const { rows } = await pool.query<Schedule>(
      `UPDATE schedules SET ${setClause}, updated_at = now() WHERE id = $1 RETURNING *`,
      [id, ...values]
    );

    const schedule = rows[0]!;
    if (!schedule) throw new Error('Schedule not found');

    if (this.boss && schedule.type === 'cron') {
      if (schedule.status === 'active') {
        await this.ensureQueueAndSchedule(schedule.id, schedule.expression);
      } else {
        await this.boss.unschedule(schedule.id).catch(() => {});
      }
    }

    // Recompute next_run_at when expression or status changes
    if (
      (updates.expression || updates.status) &&
      schedule.type === 'cron' &&
      schedule.status === 'active'
    ) {
      const nextRunAt = computeNextRunAt(schedule.expression);
      if (nextRunAt) {
        await pool.query('UPDATE schedules SET next_run_at = $1 WHERE id = $2', [
          nextRunAt,
          schedule.id,
        ]);
        schedule.next_run_at = nextRunAt;
      }
    } else if (schedule.status !== 'active') {
      await pool.query('UPDATE schedules SET next_run_at = NULL WHERE id = $1', [schedule.id]);
      delete schedule.next_run_at;
    }

    await AuditService.getInstance().record({
      actorType: 'system',
      actorId: 'system',
      actingContext: null,
      eventType: 'schedule.updated',
      payload: { scheduleId: schedule.id, updates: fields },
    });

    return schedule;
  }

  public async deleteSchedule(id: string): Promise<void> {
    const { rows } = await pool.query(
      'DELETE FROM schedules WHERE id = $1 RETURNING source, agent_instance_id, name',
      [id]
    );
    const deleted = rows[0];

    if (deleted && deleted.source === 'manifest') {
      // Re-insert if it's a manifest schedule? No, the instructions say manifest schedules cannot be deleted via API.
      // I should check this in the route handler.
    }

    if (this.boss) {
      await this.boss.unschedule(id);
    }

    if (deleted) {
      await AuditService.getInstance().record({
        actorType: 'system',
        actorId: 'system',
        actingContext: null,
        eventType: 'schedule.deleted',
        payload: { scheduleId: id, name: deleted.name, agentId: deleted.agent_instance_id },
      });
    }
  }

  public async triggerSchedule(
    id: string,
    force = false
  ): Promise<{ status: 'triggered' | 'skipped'; reason?: string }> {
    const { rows } = await pool.query<Schedule>('SELECT * FROM schedules WHERE id = $1', [id]);
    const schedule = rows[0];
    if (!schedule) throw new Error('Schedule not found');

    // schedule.task is JSONB — PostgreSQL deserializes it to a JS object on SELECT.
    // Extract the prompt string for downstream consumers that expect plain text.
    const taskPrompt = resolveTaskPrompt(schedule.task);

    logger.info(`Triggering schedule ${schedule.name} for agent ${schedule.agent_name}`);

    // Orchestration task: if the task JSON has a `tool` field, route to a bridge agent's queue
    // instead of running the schedule's own agent process.
    const orchestrationResult = await maybeRouteOrchestrationTask(schedule, taskPrompt);
    if (orchestrationResult !== null) {
      await this.updateRunStatus(id, orchestrationResult.ok ? 'success' : 'error');
      if (!orchestrationResult.ok) {
        throw new Error(orchestrationResult.reason);
      }
      await AuditService.getInstance().record({
        actorType: 'system',
        actorId: 'system',
        actingContext: null,
        eventType: 'schedule.fired',
        payload: {
          scheduleId: schedule.id,
          agentId: schedule.agent_instance_id,
          orchestration: true,
          tool: orchestrationResult.tool,
          bridgeAgentId: orchestrationResult.bridgeAgentId,
        },
      });
      return { status: 'triggered' };
    }

    if (!this.orchestrator) {
      logger.error('Orchestrator not set in ScheduleService');
      throw new Error('Internal error: Orchestrator not available');
    }

    // Get agent instance to check lifecycle mode
    const agentInstance = await pool.query(
      'SELECT lifecycle_mode, status FROM agent_instances WHERE id = $1',
      [schedule.agent_instance_id]
    );

    if (agentInstance.rows.length === 0) {
      logger.error(
        `Agent instance ${schedule.agent_instance_id} not found for schedule ${schedule.id}`
      );
      throw new Error('Agent instance not found');
    }

    const { lifecycle_mode, status } = agentInstance.rows[0];

    // Dedup guard for persistent agents: skip only if THIS schedule already has a pending task.
    // Other schedules can enqueue freely — the agent processes them sequentially.
    if (lifecycle_mode === 'persistent') {
      if (!force) {
        const existingTask = await pool.query(
          `SELECT id FROM task_queue
           WHERE agent_instance_id = $1
             AND status IN ('queued', 'running')
             AND context->'schedule'->>'scheduleId' = $2
           LIMIT 1`,
          [schedule.agent_instance_id, schedule.id]
        );
        if (existingTask.rows.length > 0) {
          logger.info(
            `Schedule ${schedule.name} already has a queued/running task — skipping duplicate`
          );
          await this.updateRunStatus(id, 'skipped', 'Schedule already has a pending task');
          return { status: 'skipped', reason: 'Schedule already has a pending task' };
        }
      }

      // Enqueue task with schedule metadata in context
      const taskId = uuidv4();
      const scheduleContext = JSON.stringify({
        schedule: {
          scheduleId: schedule.id,
          scheduleName: schedule.name,
          category: schedule.category ?? null,
          firedAt: new Date().toISOString(),
        },
      });
      await pool.query(
        `INSERT INTO task_queue (id, agent_instance_id, task, context, status) VALUES ($1, $2, $3, $4, 'queued')`,
        [taskId, schedule.agent_instance_id, taskPrompt, scheduleContext]
      );

      // If agent not running, start it
      if (status !== 'running') {
        await this.orchestrator.startInstance(schedule.agent_instance_id).catch((err) => {
          logger.error(`Failed to start agent ${schedule.agent_name} for schedule:`, err);
        });
      }
    } else {
      // Ephemeral agent
      // Check if already running (Story 11.2 skip fire)
      if (status === 'running' && !force) {
        logger.warn(
          `Skipping scheduled task for ${schedule.agent_name}: ephemeral agent is already running.`
        );
        await this.updateRunStatus(id, 'skipped', 'Agent already running');
        return { status: 'skipped', reason: 'Agent already running' };
      }

      // Start with task
      // Note: We need to update Orchestrator.startInstance to accept a task
      await this.orchestrator
        .startInstance(schedule.agent_instance_id, undefined, taskPrompt)
        .catch((err: Error) => {
          logger.error(`Failed to spawn ephemeral agent ${schedule.agent_name} for schedule:`, err);
        });
    }

    await this.updateRunStatus(id, 'success');

    await AuditService.getInstance().record({
      actorType: 'system',
      actorId: 'system',
      actingContext: null,
      eventType: 'schedule.fired',
      payload: { scheduleId: schedule.id, agentId: schedule.agent_instance_id },
    });

    return { status: 'triggered' };
  }

  private async updateRunStatus(
    id: string,
    runStatus: string,
    errorMessage?: string
  ): Promise<void> {
    // Fetch the schedule to recompute next_run_at for cron schedules
    const { rows } = await pool.query<Schedule>('SELECT * FROM schedules WHERE id = $1', [id]);
    const schedule = rows[0];

    let nextRunAt: Date | null = null;
    if (schedule && schedule.type === 'cron' && runStatus === 'success') {
      nextRunAt = computeNextRunAt(schedule.expression);
    }

    await pool.query(
      `UPDATE schedules SET
        last_run_at = now(),
        last_run_status = $1,
        next_run_at = $3,
        status = CASE WHEN type = 'once' AND $1 = 'success' THEN 'completed' ELSE status END
       WHERE id = $2`,
      [errorMessage || runStatus, id, nextRunAt]
    );
  }

  /**
   * Upserts a manifest-sourced schedule. Updates existing manifest schedules
   * in-place; never overwrites operator-created API schedules.
   */
  public async upsertManifestSchedule(data: {
    agent_instance_id: string;
    agent_name: string;
    name: string;
    description?: string;
    type: 'cron' | 'once';
    expression: string;
    task: string;
    status: 'active' | 'paused' | 'completed' | 'error';
    category?: string;
  }): Promise<Schedule> {
    // Validate cron expression
    if (data.type === 'cron') {
      const cronError = validateCronExpression(data.expression);
      if (cronError) throw new Error(`Invalid cron expression: ${cronError}`);
    }

    const taskJson = normalizeTaskJson(data.task);
    const nextRunAt =
      data.type === 'cron' && data.status === 'active' ? computeNextRunAt(data.expression) : null;

    const id = uuidv4();
    const { rows } = await pool.query<Schedule>(
      `INSERT INTO schedules
       (id, agent_instance_id, agent_name, name, description, type, expression, task, status, source, category, next_run_at)
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'manifest', $10, $11)
       ON CONFLICT (agent_instance_id, name) WHERE agent_instance_id IS NOT NULL
       DO UPDATE SET
         expression = EXCLUDED.expression,
         task = EXCLUDED.task,
         status = EXCLUDED.status,
         category = EXCLUDED.category,
         description = EXCLUDED.description,
         type = EXCLUDED.type,
         next_run_at = EXCLUDED.next_run_at,
         updated_at = now()
       WHERE schedules.source = 'manifest'
       RETURNING *`,
      [
        id,
        data.agent_instance_id,
        data.agent_name,
        data.name,
        data.description ?? null,
        data.type,
        data.expression,
        taskJson,
        data.status,
        data.category ?? null,
        nextRunAt,
      ]
    );

    const schedule = rows[0];
    if (!schedule) {
      // ON CONFLICT matched but WHERE source='manifest' failed — an API schedule exists with same name.
      // Return the existing schedule unchanged.
      const { rows: existing } = await pool.query<Schedule>(
        'SELECT * FROM schedules WHERE agent_instance_id = $1 AND name = $2',
        [data.agent_instance_id, data.name]
      );
      return existing[0]!;
    }

    // Register/unregister in pg-boss
    if (this.boss && schedule.type === 'cron') {
      if (schedule.status === 'active') {
        await this.ensureQueueAndSchedule(schedule.id, schedule.expression);
      } else {
        await this.boss.unschedule(schedule.id).catch(() => {});
      }
    }

    return schedule;
  }

  /**
   * Removes manifest schedules that are no longer in the template.
   * Only deletes schedules where source='manifest'.
   */
  public async removeStaleManifestSchedules(
    agentInstanceId: string,
    currentManifestNames: string[]
  ): Promise<void> {
    let query: string;
    let params: unknown[];

    if (currentManifestNames.length === 0) {
      // No manifest schedules — remove all manifest-sourced schedules for this instance
      query = `SELECT id, name FROM schedules WHERE agent_instance_id = $1 AND source = 'manifest'`;
      params = [agentInstanceId];
    } else {
      // Remove manifest schedules whose name is not in the current list
      const placeholders = currentManifestNames.map((_, i) => `$${i + 2}`).join(', ');
      query = `SELECT id, name FROM schedules WHERE agent_instance_id = $1 AND source = 'manifest' AND name NOT IN (${placeholders})`;
      params = [agentInstanceId, ...currentManifestNames];
    }

    const { rows: stale } = await pool.query<{ id: string; name: string }>(query, params);

    await Promise.all(
      stale.map(async (row) => {
        if (this.boss) {
          await this.boss.unschedule(row.id);
        }
        await pool.query('DELETE FROM schedules WHERE id = $1', [row.id]);
        logger.info(`Removed stale manifest schedule "${row.name}" (${row.id})`);

        await AuditService.getInstance().record({
          actorType: 'system',
          actorId: 'system',
          actingContext: null,
          eventType: 'schedule.deleted',
          payload: {
            scheduleId: row.id,
            name: row.name,
            agentId: agentInstanceId,
            reason: 'removed_from_manifest',
          },
        });
      })
    );
  }

  /**
   * Ensures the pg-boss queue exists before scheduling a cron job.
   * pg-boss v9+ requires createQueue() before schedule().
   */
  private async ensureQueueAndSchedule(scheduleId: string, expression: string): Promise<void> {
    if (!this.boss) return;
    await this.boss.createQueue(scheduleId);
    await this.boss.schedule(scheduleId, expression, { scheduleId });
  }

  public async stop(): Promise<void> {
    // PgBoss lifecycle is managed by PgBossService singleton
    this.boss = null;
    this.initialized = false;
  }
}

type OrchestrationResult =
  | { ok: true; tool: string; bridgeAgentId: string }
  | { ok: false; reason: string; tool?: string; bridgeAgentId?: never };

/**
 * If the schedule task JSON contains a `tool` field, route it as an orchestration task
 * by inserting into the bridge agent's task queue and emitting `task.assigned`.
 * Returns null if this is not an orchestration task (no `tool` field).
 */
async function maybeRouteOrchestrationTask(
  schedule: Schedule,
  taskPrompt: string
): Promise<OrchestrationResult | null> {
  // Parse the task JSON to check for a `tool` field
  const taskObj: unknown =
    typeof schedule.task === 'object' && schedule.task !== null
      ? schedule.task
      : (() => {
          try {
            return JSON.parse(String(schedule.task));
          } catch {
            return null;
          }
        })();

  if (!taskObj || typeof taskObj !== 'object') return null;
  const tool = (taskObj as Record<string, unknown>)['tool'];
  if (typeof tool !== 'string' || !tool) return null;

  const bridgeName = `${tool}-bridge`;

  // Resolve the bridge agent ID
  const { rows: agentRows } = await pool.query<{ id: string }>(
    'SELECT id FROM agent_instances WHERE name = $1 LIMIT 1',
    [bridgeName]
  );
  const bridgeAgent = agentRows[0];
  if (!bridgeAgent) {
    logger.error(`Orchestration task: bridge agent "${bridgeName}" not found`);
    return { ok: false, reason: `Bridge agent "${bridgeName}" not found`, tool };
  }

  const taskId = uuidv4();
  const scheduleContext = JSON.stringify({
    schedule: {
      scheduleId: schedule.id,
      scheduleName: schedule.name,
      category: schedule.category ?? null,
      firedAt: new Date().toISOString(),
    },
  });

  await pool.query(
    `INSERT INTO task_queue (id, agent_instance_id, task, context, status) VALUES ($1, $2, $3, $4, 'queued')`,
    [taskId, bridgeAgent.id, taskPrompt, scheduleContext]
  );

  ChannelRouter.getInstance().route({
    id: uuidv4(),
    eventType: 'task.assigned',
    title: `Orchestration task assigned to ${bridgeName}`,
    body: taskPrompt.substring(0, 200),
    severity: 'info',
    metadata: {
      taskId,
      agentId: bridgeAgent.id,
      prompt: taskPrompt.substring(0, 200),
      tool,
      scheduleId: schedule.id,
    },
    timestamp: new Date().toISOString(),
  });

  logger.info(
    `Orchestration task ${taskId} enqueued for bridge agent "${bridgeName}" (tool: ${tool})`
  );
  return { ok: true, tool, bridgeAgentId: bridgeAgent.id };
}

/** Normalize a task value to valid JSON for the JSONB column. */
function normalizeTaskJson(task: unknown): string {
  if (typeof task === 'string') {
    try {
      JSON.parse(task);
      return task; // Already valid JSON
    } catch {
      return JSON.stringify({ prompt: task }); // Wrap plain string
    }
  }
  return JSON.stringify(task);
}

/**
 * Resolves a schedule task (JSONB-deserialized) to a plain prompt string.
 * Handles: string, {prompt: "..."}, or arbitrary object (re-serialized to JSON).
 */
function resolveTaskPrompt(task: unknown): string {
  if (typeof task === 'string') return task;
  if (task && typeof task === 'object') {
    const obj = task as Record<string, unknown>;
    if (typeof obj.prompt === 'string') return obj.prompt;
    return JSON.stringify(task);
  }
  return String(task);
}

function uuidValidate(uuid: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(uuid);
}
