import type { PgBoss } from 'pg-boss';
import { pool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';
import type { Orchestrator } from '../agents/Orchestrator.js';
import { AuditService } from '../audit/AuditService.js';
import { v4 as uuidv4 } from 'uuid';

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
    for (const schedule of dbSchedules) {
      try {
        await this.boss.schedule(schedule.id, schedule.expression, { scheduleId: schedule.id });

        // Update next_run_at in DB
        // pg-boss doesn't easily expose next run time, but we can compute it if needed
        // For now, we rely on pg-boss to fire it.
      } catch (err) {
        logger.error(`Failed to schedule ${schedule.name} (${schedule.id}):`, err);
        await pool.query(
          "UPDATE schedules SET status = 'error', last_run_status = $1 WHERE id = $2",
          [(err as Error).message, schedule.id]
        );
      }
    }

    // 3. Remove stale schedules from pg-boss
    // We can query pgboss.schedule table directly
    const { rows: pgbossSchedules } = await pool.query(
      "SELECT name FROM pgboss.schedule WHERE name NOT IN (SELECT id::text FROM schedules WHERE type = 'cron' AND status = 'active')"
    );

    for (const row of pgbossSchedules) {
      // Ignore internal jobs like 'memory-compaction'
      if (uuidValidate(row.name)) {
        logger.info(`Removing stale pg-boss schedule: ${row.name}`);
        await this.boss.unschedule(row.name);
      }
    }
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

    // Normalize task to valid JSON for the JSONB column.
    // Plain strings (e.g. from template YAML) are wrapped as { prompt: "..." }.
    let taskJson: string;
    if (typeof task === 'string') {
      try {
        JSON.parse(task);
        taskJson = task; // Already valid JSON
      } catch {
        taskJson = JSON.stringify({ prompt: task }); // Wrap plain string
      }
    } else {
      taskJson = JSON.stringify(task);
    }

    const { rows } = await pool.query<Schedule>(
      `INSERT INTO schedules
       (id, agent_instance_id, agent_name, name, type, expression, task, status, source, category)
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
       RETURNING *`,
      [
        id,
        agent_instance_id,
        agent_name,
        name,
        type,
        expression,
        taskJson,
        initialStatus,
        source,
        category ?? null,
      ]
    );

    const schedule = rows[0]!;

    if (this.boss && schedule.type === 'cron' && schedule.status === 'active') {
      await this.boss.schedule(schedule.id, schedule.expression, { scheduleId: schedule.id });
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
      ['name', 'expression', 'task', 'status', 'category'].includes(k)
    );

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
        await this.boss.schedule(schedule.id, schedule.expression, { scheduleId: schedule.id });
      } else {
        await this.boss.unschedule(schedule.id);
      }
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

  public async triggerSchedule(id: string): Promise<void> {
    const { rows } = await pool.query<Schedule>('SELECT * FROM schedules WHERE id = $1', [id]);
    const schedule = rows[0];
    if (!schedule) return;

    logger.info(`Triggering schedule ${schedule.name} for agent ${schedule.agent_name}`);

    if (!this.orchestrator) {
      logger.error('Orchestrator not set in ScheduleService');
      return;
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
      return;
    }

    const { lifecycle_mode, status } = agentInstance.rows[0];

    // Concurrent execution guard
    // For persistent agents, check task_queue
    if (lifecycle_mode === 'persistent') {
      const runningTask = await pool.query(
        "SELECT id FROM task_queue WHERE agent_instance_id = $1 AND status = 'running' LIMIT 1",
        [schedule.agent_instance_id]
      );
      if (runningTask.rows.length > 0) {
        logger.warn(
          `Skipping scheduled task for ${schedule.agent_name}: agent is already running a task.`
        );
        await this.updateRunStatus(id, 'skipped', 'Agent already running a task');
        return;
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
        [taskId, schedule.agent_instance_id, schedule.task, scheduleContext]
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
      if (status === 'running') {
        logger.warn(
          `Skipping scheduled task for ${schedule.agent_name}: ephemeral agent is already running.`
        );
        await this.updateRunStatus(id, 'skipped', 'Agent already running');
        return;
      }

      // Start with task
      // Note: We need to update Orchestrator.startInstance to accept a task
      await this.orchestrator
        .startInstance(schedule.agent_instance_id, undefined, schedule.task)
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
  }

  private async updateRunStatus(
    id: string,
    runStatus: string,
    errorMessage?: string
  ): Promise<void> {
    await pool.query(
      `UPDATE schedules SET 
        last_run_at = now(), 
        last_run_status = $1,
        status = CASE WHEN type = 'once' AND $1 = 'success' THEN 'completed' ELSE status END
       WHERE id = $2`,
      [errorMessage || runStatus, id]
    );
  }

  public async stop(): Promise<void> {
    // PgBoss lifecycle is managed by PgBossService singleton
    this.boss = null;
    this.initialized = false;
  }
}

function uuidValidate(uuid: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(uuid);
}
