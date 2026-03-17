/**
 * Session Types — Persistent chat session management.
 *
 * Sessions store conversation history per agent in PostgreSQL,
 * with JSONL disk mirrors for human readability.
 */

// ── Session ─────────────────────────────────────────────────────────────────────

/** A chat session belonging to a specific agent. */
export interface ChatSession {
  /** Stable UUID. */
  id: string;
  /** Which agent this session belongs to. */
  agentName: string;
  /** Optional agent instance ID. */
  agentInstanceId: string | undefined;
  /** Human-readable title (auto-generated from first message, editable). */
  title: string;
  /** Denormalized message count for list views. */
  messageCount: number;
  /** ISO-8601 timestamp. */
  createdAt: string;
  /** ISO-8601 timestamp. */
  updatedAt: string;
}

// ── Message ─────────────────────────────────────────────────────────────────────

export type MessageRole = 'user' | 'assistant' | 'system' | 'tool';

/** A single message within a session. */
export interface SessionMessage {
  /** Stable UUID. */
  id: string;
  /** FK → ChatSession.id. */
  sessionId: string;
  /** Message role. */
  role: MessageRole;
  /** Message text content. */
  content: string;
  /** Optional metadata (thoughts, tool calls, etc.). */
  metadata: Record<string, unknown> | undefined;
  /** ISO-8601 timestamp. */
  createdAt: string;
}

// ── Create Options ──────────────────────────────────────────────────────────────

export interface CreateSessionOptions {
  id?: string;
  agentName: string;
  agentInstanceId?: string | undefined;
  title?: string;
}

export interface AddMessageOptions {
  sessionId: string;
  role: MessageRole;
  content: string;
  metadata?: Record<string, unknown>;
}
