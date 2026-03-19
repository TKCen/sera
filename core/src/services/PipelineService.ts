import { v4 as uuidv4 } from 'uuid';
import { query } from '../lib/database.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('PipelineService');

export interface PipelineStep {
  agentId?: string;
  description: string;
  status: 'pending' | 'running' | 'completed' | 'failed';
  result?: string;
  error?: string;
  startedAt?: string;
  completedAt?: string;
}

export interface Pipeline {
  id: string;
  type: 'sequential' | 'parallel' | 'hierarchical';
  status: 'pending' | 'running' | 'completed' | 'failed';
  steps: PipelineStep[];
  createdAt: string;
  completedAt?: string;
}

export class PipelineService {
  private static instance: PipelineService;

  private constructor() {}

  static getInstance(): PipelineService {
    if (!PipelineService.instance) {
      PipelineService.instance = new PipelineService();
    }
    return PipelineService.instance;
  }

  async create(type: Pipeline['type'], steps: PipelineStep[]): Promise<Pipeline> {
    const id = uuidv4();
    const initialSteps: PipelineStep[] = steps.map(s => ({ ...s, status: 'pending' as const }));

    await query(
      `INSERT INTO pipelines (id, type, status, steps) VALUES ($1, $2, 'pending', $3)`,
      [id, type, JSON.stringify(initialSteps)],
    );

    logger.info(`Created pipeline ${id} (${type}, ${steps.length} steps)`);
    return this.get(id) as Promise<Pipeline>;
  }

  async get(id: string): Promise<Pipeline | null> {
    const result = await query(`SELECT * FROM pipelines WHERE id = $1`, [id]);
    if (result.rows.length === 0) return null;
    const row = result.rows[0];
    const completedAt = row.completed_at ? (row.completed_at as Date).toISOString() : undefined;
    return {
      id: row.id as string,
      type: row.type as Pipeline['type'],
      status: row.status as Pipeline['status'],
      steps: (row.steps ?? []) as PipelineStep[],
      createdAt: (row.created_at as Date).toISOString(),
      ...(completedAt !== undefined ? { completedAt } : {}),
    };
  }

  async updateStatus(id: string, status: Pipeline['status']): Promise<void> {
    if (status === 'completed' || status === 'failed') {
      await query(
        `UPDATE pipelines SET status = $1, completed_at = NOW() WHERE id = $2`,
        [status, id],
      );
    } else {
      await query(`UPDATE pipelines SET status = $1 WHERE id = $2`, [status, id]);
    }
  }

  async updateSteps(id: string, steps: PipelineStep[]): Promise<void> {
    await query(`UPDATE pipelines SET steps = $1 WHERE id = $2`, [JSON.stringify(steps), id]);
  }
}
