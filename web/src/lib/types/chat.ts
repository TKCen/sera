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
