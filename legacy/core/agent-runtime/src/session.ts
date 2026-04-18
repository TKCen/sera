/**
 * SessionStore — serialize, save, load, and clean up agent reasoning sessions.
 *
 * Persists session state to the agent workspace bind mount so that
 * conversations survive container restarts.
 */

import fs from 'fs';
import path from 'path';
import type { ChatMessage, ToolCall } from './llmClient.js';
import { log } from './logger.js';

// ── Types ────────────────────────────────────────────────────────────────────

export interface SerializedMessage {
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  tool_calls?: ToolCall[];
  tool_call_id?: string;
  usage?: {
    promptTokens: number;
    completionTokens: number;
  };
}

export interface SerializedSession {
  version: number;
  agentId: string;
  taskId: string;
  messages: SerializedMessage[];
  totalUsage: { promptTokens: number; completionTokens: number };
  createdAt: string;
  updatedAt: string;
}

// ── Constants ────────────────────────────────────────────────────────────────

const SESSION_VERSION = 1;
const SERA_DIR = '.sera';
const SESSION_FILE = 'session.json';
const DEBOUNCE_MS = 5_000;
const STALE_SESSION_MS = 24 * 60 * 60 * 1000; // 24 hours

// ── SessionStore ─────────────────────────────────────────────────────────────

export class SessionStore {
  private sessionPath: string;
  private debounceTimer: ReturnType<typeof setTimeout> | null = null;
  private lastSaveTime = 0;

  constructor(workspacePath: string) {
    const seraDir = path.join(workspacePath, SERA_DIR);
    this.sessionPath = path.join(seraDir, SESSION_FILE);
  }

  /**
   * Serialize ChatMessage[] into a portable format.
   * Strips internal-only fields and normalizes content to string.
   */
  serialize(
    messages: ChatMessage[],
    agentId: string,
    taskId: string,
    usage: { promptTokens: number; completionTokens: number },
    createdAt?: string
  ): SerializedSession {
    const serializedMessages: SerializedMessage[] = messages
      .filter((m) => !m.internal)
      .map((m) => {
        const sm: SerializedMessage = {
          role: m.role,
          content:
            typeof m.content === 'string'
              ? m.content
              : m.content
                  .map((block) =>
                    block.type === 'text' && block.text !== undefined ? block.text : ''
                  )
                  .join(''),
        };
        if (m.tool_calls && m.tool_calls.length > 0) {
          sm.tool_calls = m.tool_calls;
        }
        if (m.tool_call_id) {
          sm.tool_call_id = m.tool_call_id;
        }
        return sm;
      });

    return {
      version: SESSION_VERSION,
      agentId,
      taskId,
      messages: serializedMessages,
      totalUsage: usage,
      createdAt: createdAt ?? new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };
  }

  /**
   * Deserialize a SerializedSession back into ChatMessage[].
   */
  deserialize(session: SerializedSession): ChatMessage[] {
    return session.messages.map((sm) => {
      const msg: ChatMessage = {
        role: sm.role,
        content: sm.content,
      };
      if (sm.tool_calls && sm.tool_calls.length > 0) {
        msg.tool_calls = sm.tool_calls;
      }
      if (sm.tool_call_id) {
        msg.tool_call_id = sm.tool_call_id;
      }
      return msg;
    });
  }

  /**
   * Save session to disk immediately.
   */
  saveSync(session: SerializedSession): void {
    try {
      const dir = path.dirname(this.sessionPath);
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
      }
      const data = JSON.stringify(session, null, 2);
      fs.writeFileSync(this.sessionPath, data, 'utf-8');
      this.lastSaveTime = Date.now();
      log('debug', `Session saved to ${this.sessionPath}`);
    } catch (err) {
      log('warn', `Failed to save session: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  /**
   * Debounced save — writes at most once per DEBOUNCE_MS.
   */
  save(session: SerializedSession): void {
    if (this.debounceTimer) {
      clearTimeout(this.debounceTimer);
    }

    const elapsed = Date.now() - this.lastSaveTime;
    if (elapsed >= DEBOUNCE_MS) {
      // Enough time has passed, save immediately
      this.saveSync(session);
      return;
    }

    // Schedule save after remaining debounce time
    this.debounceTimer = setTimeout(() => {
      this.debounceTimer = null;
      this.saveSync(session);
    }, DEBOUNCE_MS - elapsed);
  }

  /**
   * Flush any pending debounced save immediately.
   */
  flush(): void {
    if (this.debounceTimer) {
      clearTimeout(this.debounceTimer);
      this.debounceTimer = null;
    }
  }

  /**
   * Load a saved session from disk. Returns null if no session file exists
   * or if the file is invalid.
   */
  load(): SerializedSession | null {
    try {
      if (!fs.existsSync(this.sessionPath)) {
        return null;
      }

      const data = fs.readFileSync(this.sessionPath, 'utf-8');
      const session = JSON.parse(data) as SerializedSession;

      if (session.version !== SESSION_VERSION) {
        log(
          'warn',
          `Session version mismatch: expected ${SESSION_VERSION}, got ${session.version}`
        );
        return null;
      }

      log('info', `Session loaded from ${this.sessionPath} (${session.messages.length} messages)`);
      return session;
    } catch (err) {
      log('warn', `Failed to load session: ${err instanceof Error ? err.message : String(err)}`);
      return null;
    }
  }

  /**
   * Delete session file on successful task completion.
   */
  delete(): void {
    try {
      if (fs.existsSync(this.sessionPath)) {
        fs.unlinkSync(this.sessionPath);
        log('info', `Session file deleted: ${this.sessionPath}`);
      }
    } catch (err) {
      log('warn', `Failed to delete session: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  /**
   * Clean up stale sessions (>24h old) on startup.
   */
  cleanup(): void {
    try {
      if (!fs.existsSync(this.sessionPath)) {
        return;
      }

      const stat = fs.statSync(this.sessionPath);
      const age = Date.now() - stat.mtimeMs;

      if (age > STALE_SESSION_MS) {
        fs.unlinkSync(this.sessionPath);
        log('info', `Cleaned up stale session (${Math.round(age / 3600000)}h old)`);
      }
    } catch (err) {
      log(
        'warn',
        `Failed to clean up session: ${err instanceof Error ? err.message : String(err)}`
      );
    }
  }
}
