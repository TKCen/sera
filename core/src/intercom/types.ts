/**
 * Intercom type definitions — standard envelope schema for all
 * Centrifugo-based messaging within SERA.
 *
 * @see sera/docs/reimplementation/agent-workspace-architecture.md §Intercom Architecture
 */

// ── Message Types ───────────────────────────────────────────────────────────────

export type IntercomMessageType =
  | 'message'
  | 'knowledge'
  | 'task'
  | 'status'
  | 'approval-request'
  | 'thought'
  | 'usage';

// ── Thought Step Types ──────────────────────────────────────────────────────────

export type ThoughtStepType =
  | 'observe'
  | 'plan'
  | 'act'
  | 'reflect'
  | 'tool-call'
  | 'tool-result'
  | 'reasoning'
  | 'context-assembly';

// ── Message Envelope ────────────────────────────────────────────────────────────

export interface MessageSource {
  agent: string;
  circle: string;
  instance?: string;
}

export interface MessageTarget {
  channel: string;
}

export interface MessageMetadata {
  securityTier: number;
  replyTo?: string;
  ttl?: number;
}

export interface IntercomMessage {
  id: string;
  version: string;
  timestamp: string;
  source: MessageSource;
  target: MessageTarget;
  type: IntercomMessageType;
  payload: Record<string, unknown>;
  metadata: MessageMetadata;
}

// ── Thought Event (published to internal:agent:{id}:thoughts) ───────────────────

export interface ThoughtEvent {
  timestamp: string;
  stepType: ThoughtStepType;
  content: string;
  agentId: string;
  agentDisplayName: string;
  taskId?: string | undefined;
  iteration?: number | undefined;
}

// ── Stream Token (published to internal:stream:{messageId}) ─────────────────────

export interface StreamToken {
  token: string;
  done: boolean;
  messageId: string;
}

// ── Channel Namespace Prefixes ──────────────────────────────────────────────────

export const CHANNEL_PREFIXES = [
  'thoughts',
  'tokens',
  'agent',
  'private',
  'circle',
  'system',
  'bridge',
  'federation',
  'usage',
] as const;

export type ChannelPrefix = (typeof CHANNEL_PREFIXES)[number];
