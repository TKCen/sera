/**
 * Shared chat types — single source of truth for Message and MessageThought.
 * Used by both the chat page and ChatThoughtPanel.
 */

export interface MessageThought {
  timestamp: string;
  stepType: string;
  content: string;
  toolName?: string;
  toolArgs?: Record<string, unknown>;
}

export interface Message {
  id: string;
  role: 'user' | 'agent';
  content: string;
  thoughts: MessageThought[];
  streaming: boolean;
  createdAt: Date;
}

export interface SessionInfo {
  id: string;
  agentName: string;
  agentInstanceId?: string | null;
  title: string;
  messageCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface SessionDetail extends SessionInfo {
  messages: SessionMessage[];
}

export interface SessionMessage {
  id: string;
  sessionId: string;
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  metadata?: { thoughts?: MessageThought[] };
  createdAt: string;
}

export interface TokenPayload {
  token: string;
  done: boolean;
  messageId?: string;
}

export interface ThoughtPayload {
  timestamp: string;
  stepType: string;
  content: string;
  agentId: string;
  agentDisplayName: string;
  toolName?: string;
  toolArgs?: Record<string, unknown>;
}
