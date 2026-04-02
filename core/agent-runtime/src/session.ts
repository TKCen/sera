import fs from 'fs';
import path from 'path';
import { log } from './logger.js';
import type { ChatMessage, LLMUsage } from './llmClient.js';

export interface SerializedMessage extends ChatMessage {}

export interface SerializedSession {
  version: number;
  agentId: string;
  taskId: string;
  iteration: number;
  messages: SerializedMessage[];
  totalUsage: LLMUsage;
  createdAt: string;
  updatedAt: string;
}

const SESSION_VERSION = 1;
const DEFAULT_SESSION_PATH = '/workspace/.sera/session.json';
const STALE_THRESHOLD_MS = 24 * 60 * 60 * 1000; // 24 hours

export class SessionStore {
  private sessionPath: string;

  constructor(sessionPath: string = DEFAULT_SESSION_PATH) {
    this.sessionPath = sessionPath;
  }

  /**
   * Save the current reasoning state to disk.
   */
  async save(session: Omit<SerializedSession, 'version' | 'updatedAt'>): Promise<void> {
    try {
      const dir = path.dirname(this.sessionPath);
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
      }

      const fullSession: SerializedSession = {
        ...session,
        version: SESSION_VERSION,
        updatedAt: new Date().toISOString(),
      };

      fs.writeFileSync(this.sessionPath, JSON.stringify(fullSession, null, 2), 'utf-8');
      log('debug', `Session saved to ${this.sessionPath} (iteration ${session.iteration})`);
    } catch (err) {
      log('warn', `Failed to save session: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  /**
   * Load session from disk if it exists and matches the current taskId.
   */
  async load(taskId: string): Promise<SerializedSession | null> {
    try {
      if (!fs.existsSync(this.sessionPath)) {
        return null;
      }

      const raw = fs.readFileSync(this.sessionPath, 'utf-8');
      const session = JSON.parse(raw) as SerializedSession;

      if (session.taskId !== taskId) {
        log('info', `Session taskId mismatch (found ${session.taskId}, expected ${taskId}) — ignoring`);
        return null;
      }

      if (session.version !== SESSION_VERSION) {
        log('warn', `Session version mismatch (found ${session.version}, expected ${SESSION_VERSION}) — ignoring`);
        return null;
      }

      log('info', `Restored session from ${this.sessionPath} (iteration ${session.iteration})`);
      return session;
    } catch (err) {
      log('warn', `Failed to load session: ${err instanceof Error ? err.message : String(err)}`);
      return null;
    }
  }

  /**
   * Delete the session file (typically on task completion).
   */
  async delete(): Promise<void> {
    try {
      if (fs.existsSync(this.sessionPath)) {
        fs.unlinkSync(this.sessionPath);
        log('debug', `Session file deleted: ${this.sessionPath}`);
      }
    } catch (err) {
      log('warn', `Failed to delete session file: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  /**
   * Remove session files older than 24h.
   */
  async cleanupStale(): Promise<void> {
    try {
      if (!fs.existsSync(this.sessionPath)) return;

      const stats = fs.statSync(this.sessionPath);
      const age = Date.now() - stats.mtimeMs;

      if (age > STALE_THRESHOLD_MS) {
        log('info', `Cleaning up stale session file (age: ${Math.round(age / 3600000)}h)`);
        await this.delete();
      }
    } catch (err) {
      log('warn', `Failed to cleanup stale session: ${err instanceof Error ? err.message : String(err)}`);
    }
  }
}
