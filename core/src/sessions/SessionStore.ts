/**
 * Session Store — PostgreSQL-backed session persistence with JSONL disk mirrors.
 *
 * Follows the hybrid pattern from OpenFang: PostgreSQL for structured queries,
 * JSONL files on disk for human readability and git-friendliness.
 */

import path from 'path';
import fs from 'fs/promises';
import { v4 as uuidv4 } from 'uuid';
import { query } from '../lib/database.js';
import { Logger } from '../lib/logger.js';
import type {
  ChatSession,
  SessionMessage,
  CreateSessionOptions,
  AddMessageOptions,
} from './types.js';

const logger = new Logger('SessionStore');

export class SessionStore {
  private readonly sessionsBasePath: string;

  /**
   * @param basePath Root path for JSONL mirror files (e.g. `/app/workspace/memory`).
   *                 Sessions are written to `{basePath}/agents/{agentName}/sessions/`.
   */
  constructor(basePath?: string) {
    this.sessionsBasePath =
      basePath ?? process.env.MEMORY_PATH ?? path.join(process.cwd(), '..', 'memory');
  }

  // ── Session CRUD ──────────────────────────────────────────────────────────

  async createSession(opts: CreateSessionOptions): Promise<ChatSession> {
    const id = opts.id || uuidv4();
    const title = opts.title || 'New Chat';
    const now = new Date().toISOString();

    await query(
      `INSERT INTO chat_sessions (id, agent_name, agent_instance_id, title, message_count, created_at, updated_at)
       VALUES ($1, $2, $3, $4, 0, $5, $5)`,
      [id, opts.agentName, opts.agentInstanceId || null, title, now]
    );

    return {
      id,
      agentName: opts.agentName,
      agentInstanceId: opts.agentInstanceId,
      title,
      messageCount: 0,
      createdAt: now,
      updatedAt: now,
    };
  }

  async getSession(id: string): Promise<ChatSession | null> {
    const result = await query(
      `SELECT id, agent_name, agent_instance_id, title, message_count, created_at, updated_at
       FROM chat_sessions WHERE id = $1`,
      [id]
    );
    if (result.rows.length === 0) return null;
    return this.rowToSession(result.rows[0]);
  }

  async listSessions(agentName?: string, agentInstanceId?: string): Promise<ChatSession[]> {
    let result;
    if (agentInstanceId) {
      result = await query(
        `SELECT id, agent_name, agent_instance_id, title, message_count, created_at, updated_at
         FROM chat_sessions WHERE agent_instance_id = $1
         ORDER BY updated_at DESC`,
        [agentInstanceId]
      );
    } else if (agentName) {
      result = await query(
        `SELECT id, agent_name, agent_instance_id, title, message_count, created_at, updated_at
         FROM chat_sessions WHERE agent_name = $1
         ORDER BY updated_at DESC`,
        [agentName]
      );
    } else {
      result = await query(
        `SELECT id, agent_name, agent_instance_id, title, message_count, created_at, updated_at
         FROM chat_sessions ORDER BY updated_at DESC`
      );
    }
    return result.rows.map((row) => this.rowToSession(row as Record<string, unknown>));
  }

  async updateSessionTitle(id: string, title: string): Promise<ChatSession | null> {
    const result = await query(
      `UPDATE chat_sessions SET title = $1, updated_at = $2
       WHERE id = $3
       RETURNING id, agent_name, title, message_count, created_at, updated_at`,
      [title, new Date().toISOString(), id]
    );
    if (result.rows.length === 0) return null;
    return this.rowToSession(result.rows[0]);
  }

  async deleteSession(id: string): Promise<boolean> {
    // Get agent name before deletion for JSONL cleanup
    const session = await this.getSession(id);

    const result = await query(`DELETE FROM chat_sessions WHERE id = $1`, [id]);

    // Best-effort: remove JSONL mirror
    if (session && result.rowCount && result.rowCount > 0) {
      this.removeJsonlMirror(session.agentName, id).catch((err: unknown) => {
        logger.warn(`Failed to remove JSONL mirror for session ${id}:`, (err as Error).message);
      });
    }

    return (result.rowCount ?? 0) > 0;
  }

  // ── Message Operations ────────────────────────────────────────────────────

  async addMessage(opts: AddMessageOptions): Promise<SessionMessage> {
    const id = uuidv4();
    const now = new Date().toISOString();

    await query(
      `INSERT INTO chat_messages (id, session_id, role, content, metadata, created_at)
       VALUES ($1, $2, $3, $4, $5, $6)`,
      [
        id,
        opts.sessionId,
        opts.role,
        opts.content,
        opts.metadata ? JSON.stringify(opts.metadata) : null,
        now,
      ]
    );

    // Update session's message_count and updated_at
    await query(
      `UPDATE chat_sessions
       SET message_count = message_count + 1, updated_at = $1
       WHERE id = $2`,
      [now, opts.sessionId]
    );

    return {
      id,
      sessionId: opts.sessionId,
      role: opts.role,
      content: opts.content,
      metadata: opts.metadata,
      createdAt: now,
    };
  }

  async getMessages(sessionId: string): Promise<SessionMessage[]> {
    const result = await query(
      `SELECT id, session_id, role, content, metadata, created_at
       FROM chat_messages WHERE session_id = $1
       ORDER BY created_at ASC`,
      [sessionId]
    );
    return result.rows.map((row) => this.rowToMessage(row as Record<string, unknown>));
  }

  // ── JSONL Disk Mirror ─────────────────────────────────────────────────────

  /**
   * Write/update a JSONL mirror file for a session.
   * Best-effort: errors are logged but never block the primary store.
   */
  async writeJsonlMirror(agentName: string, sessionId: string): Promise<void> {
    try {
      const messages = await this.getMessages(sessionId);
      const sessionsDir = path.join(this.sessionsBasePath, 'agents', agentName, 'sessions');
      await fs.mkdir(sessionsDir, { recursive: true });

      const filePath = path.join(sessionsDir, `${sessionId}.jsonl`);
      const lines = messages.map((msg) =>
        JSON.stringify({
          timestamp: msg.createdAt,
          role: msg.role,
          content: msg.content,
          ...(msg.metadata ? { metadata: msg.metadata } : {}),
        })
      );

      await fs.writeFile(filePath, lines.join('\n') + '\n', 'utf-8');
    } catch (err: unknown) {
      logger.warn(`Failed to write JSONL mirror for session ${sessionId}:`, (err as Error).message);
    }
  }

  /**
   * Remove a JSONL mirror file.
   */
  private async removeJsonlMirror(agentName: string, sessionId: string): Promise<void> {
    const filePath = path.join(
      this.sessionsBasePath,
      'agents',
      agentName,
      'sessions',
      `${sessionId}.jsonl`
    );
    try {
      await fs.unlink(filePath);
    } catch {
      // File may not exist, that's fine
    }
  }

  // ── Row Mappers ───────────────────────────────────────────────────────────

  private rowToSession(row: Record<string, unknown>): ChatSession {
    return {
      id: row['id'] as string,
      agentName: row['agent_name'] as string,
      agentInstanceId: row['agent_instance_id'] as string,
      title: row['title'] as string,
      messageCount: row['message_count'] as number,
      createdAt:
        row['created_at'] instanceof Date
          ? (row['created_at'] as Date).toISOString()
          : (row['created_at'] as string),
      updatedAt:
        row['updated_at'] instanceof Date
          ? (row['updated_at'] as Date).toISOString()
          : (row['updated_at'] as string),
    };
  }

  private rowToMessage(row: Record<string, unknown>): SessionMessage {
    return {
      id: row['id'] as string,
      sessionId: row['session_id'] as string,
      role: row['role'] as SessionMessage['role'],
      content: row['content'] as string,
      metadata: (row['metadata'] as Record<string, unknown>) ?? undefined,
      createdAt:
        row['created_at'] instanceof Date
          ? (row['created_at'] as Date).toISOString()
          : (row['created_at'] as string),
    };
  }
}
