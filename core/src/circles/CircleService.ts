import { v4 as uuidv4 } from 'uuid';
import { query } from '../lib/database.js';
import { Logger } from '../lib/logger.js';
import { KnowledgeGitService } from '../memory/KnowledgeGitService.js';
import { AuditService } from '../audit/AuditService.js';

const logger = new Logger('CircleService');

export interface Circle {
  id: string;
  name: string;
  displayName: string;
  description?: string;
  constitution?: string;
  createdAt: string;
  updatedAt: string;
}

export class CircleService {
  private static instance: CircleService;
  private kgs = KnowledgeGitService.getInstance();
  private audit = AuditService.getInstance();

  private constructor() {}

  static getInstance(): CircleService {
    if (!CircleService.instance) {
      CircleService.instance = new CircleService();
    }
    return CircleService.instance;
  }

  async createCircle(data: {
    name: string;
    displayName: string;
    description?: string;
    constitution?: string;
  }): Promise<Circle> {
    const id = uuidv4();
    const now = new Date().toISOString();

    const result = await query(
      `INSERT INTO circles (id, name, display_name, description, constitution, created_at, updated_at)
       VALUES ($1, $2, $3, $4, $5, $6, $6)
       RETURNING *`,
      [id, data.name, data.displayName, data.description, data.constitution, now]
    );

    const row = result.rows[0];
    const circle: Circle = {
      id: row.id,
      name: row.name,
      displayName: row.display_name,
      description: row.description,
      constitution: row.constitution,
      createdAt: row.created_at.toISOString(),
      updatedAt: row.updated_at.toISOString(),
    };

    // Initialize knowledge repository
    await this.kgs.initCircleRepo(circle.id);

    // Audit record
    await this.audit.record({
      actorType: 'system',
      actorId: 'system',
      actingContext: null,
      eventType: 'circle.created',
      payload: { circleId: circle.id, name: circle.name, displayName: circle.displayName },
    });

    logger.info(`Created circle: ${circle.name} (${circle.id})`);
    return circle;
  }

  async getCircle(idOrName: string): Promise<Circle | null> {
    const result = await query(`SELECT * FROM circles WHERE id::text = $1 OR name = $1`, [
      idOrName,
    ]);

    if (result.rows.length === 0) return null;
    const row = result.rows[0];

    return {
      id: row.id,
      name: row.name,
      displayName: row.display_name,
      description: row.description,
      constitution: row.constitution,
      createdAt: row.created_at.toISOString(),
      updatedAt: row.updated_at.toISOString(),
    };
  }

  async listCircles(): Promise<Circle[]> {
    const result = await query(`SELECT * FROM circles ORDER BY name ASC`);
    return result.rows.map((row) => ({
      id: row.id,
      name: row.name,
      displayName: row.display_name,
      description: row.description,
      constitution: row.constitution,
      createdAt: row.created_at.toISOString(),
      updatedAt: row.updated_at.toISOString(),
    }));
  }

  async deleteCircle(id: string): Promise<void> {
    // Check for active members
    const members = await query(
      `SELECT id FROM agent_instances WHERE circle_id = $1 AND status = 'running'`,
      [id]
    );

    if (members.rows.length > 0) {
      throw new Error(
        `Cannot delete circle "${id}": it has ${members.rows.length} active agent(s)`
      );
    }

    const circle = await this.getCircle(id);
    if (!circle) return;

    await query(`DELETE FROM circles WHERE id = $1`, [id]);

    // Archive knowledge repository
    await this.kgs.archiveCircleRepo(id);

    // Audit record
    await this.audit.record({
      actorType: 'system',
      actorId: 'system',
      actingContext: null,
      eventType: 'circle.deleted',
      payload: { circleId: id },
    });

    logger.info(`Deleted circle: ${circle.name} (${id})`);
  }

  async addMember(circleId: string, agentInstanceId: string): Promise<void> {
    const circle = await this.getCircle(circleId);
    if (!circle) throw new Error(`Circle "${circleId}" not found`);

    await query(`UPDATE agent_instances SET circle_id = $1, updated_at = NOW() WHERE id = $2`, [
      circle.id,
      agentInstanceId,
    ]);

    await this.audit.record({
      actorType: 'system',
      actorId: 'system',
      actingContext: null,
      eventType: 'circle.membership_changed',
      payload: { circleId: circle.id, agentId: agentInstanceId, action: 'added' },
    });

    logger.info(`Added agent ${agentInstanceId} to circle ${circle.name}`);
  }

  async removeMember(circleId: string, agentInstanceId: string): Promise<void> {
    await query(
      `UPDATE agent_instances SET circle_id = NULL, updated_at = NOW() WHERE id = $1 AND circle_id = $2`,
      [agentInstanceId, circleId]
    );

    await this.audit.record({
      actorType: 'system',
      actorId: 'system',
      actingContext: null,
      eventType: 'circle.membership_changed',
      payload: { circleId, agentId: agentInstanceId, action: 'removed' },
    });

    logger.info(`Removed agent ${agentInstanceId} from circle ${circleId}`);
  }

  async updateCircle(
    id: string,
    data: { displayName?: string; description?: string; constitution?: string }
  ): Promise<Circle | null> {
    const setParts: string[] = [];
    const values: unknown[] = [];
    let paramIdx = 1;

    if (data.displayName !== undefined) {
      setParts.push(`display_name = $${paramIdx++}`);
      values.push(data.displayName);
    }
    if (data.description !== undefined) {
      setParts.push(`description = $${paramIdx++}`);
      values.push(data.description);
    }
    if (data.constitution !== undefined) {
      setParts.push(`constitution = $${paramIdx++}`);
      values.push(data.constitution);
    }

    if (setParts.length === 0) return this.getCircle(id);

    setParts.push(`updated_at = NOW()`);
    values.push(id);

    const result = await query(
      `UPDATE circles SET ${setParts.join(', ')} WHERE id = $${paramIdx} RETURNING *`,
      values
    );

    if (result.rows.length === 0) return null;
    const row = result.rows[0];

    await this.audit.record({
      actorType: 'system',
      actorId: 'system',
      actingContext: null,
      eventType: 'circle.updated',
      payload: { circleId: id, fields: Object.keys(data) },
    });

    return {
      id: row.id,
      name: row.name,
      displayName: row.display_name,
      description: row.description,
      constitution: row.constitution,
      createdAt: row.created_at.toISOString(),
      updatedAt: row.updated_at.toISOString(),
    };
  }

  async getMembers(circleId: string): Promise<CircleMember[]> {
    const result = await query(
      `SELECT id, name, template_name, status, circle_id, lifecycle_mode FROM agent_instances WHERE circle_id = $1`,
      [circleId]
    );
    return result.rows.map((row) => {
      const lm = row.lifecycle_mode as string | undefined;
      const member: CircleMember = {
        id: row.id as string,
        name: row.name as string,
        templateName: row.template_name as string,
        status: row.status as string,
        ...(lm !== undefined ? { lifecycleMode: lm } : {}),
      };
      return member;
    });
  }

  async createPartySession(data: { circleId: string; prompt: string }): Promise<PartySession> {
    const id = uuidv4();
    const result = await query(
      `INSERT INTO party_sessions (id, circle_id, prompt, rounds) VALUES ($1, $2, $3, '[]') RETURNING *`,
      [id, data.circleId, data.prompt]
    );
    const row = result.rows[0];
    return {
      id: row.id as string,
      circleId: row.circle_id as string,
      prompt: row.prompt as string,
      rounds: [] as PartyRound[],
      createdAt: (row.created_at as Date).toISOString(),
    };
  }

  async appendPartyRound(sessionId: string, round: PartyRound): Promise<void> {
    await query(`UPDATE party_sessions SET rounds = rounds || $1::jsonb WHERE id = $2`, [
      JSON.stringify([round]),
      sessionId,
    ]);
  }

  async completePartySession(sessionId: string): Promise<void> {
    await query(`UPDATE party_sessions SET completed_at = NOW() WHERE id = $1`, [sessionId]);
  }

  async getPartySession(sessionId: string): Promise<PartySession | null> {
    const result = await query(`SELECT * FROM party_sessions WHERE id = $1`, [sessionId]);
    if (result.rows.length === 0) return null;
    const row = result.rows[0];
    const completedAt = row.completed_at ? (row.completed_at as Date).toISOString() : undefined;
    return {
      id: row.id as string,
      circleId: row.circle_id as string,
      prompt: row.prompt as string,
      rounds: (row.rounds ?? []) as PartyRound[],
      createdAt: (row.created_at as Date).toISOString(),
      ...(completedAt !== undefined ? { completedAt } : {}),
    };
  }
}

export interface CircleMember {
  id: string;
  name: string;
  templateName: string;
  status: string;
  lifecycleMode?: string;
}

export interface PartyRound {
  agentId: string;
  response: string;
  timestamp: string;
}

export interface PartySession {
  id: string;
  circleId: string;
  prompt: string;
  rounds: PartyRound[];
  createdAt: string;
  completedAt?: string;
}
