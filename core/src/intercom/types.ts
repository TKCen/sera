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
  | 'thought';

// ── Thought Step Types ──────────────────────────────────────────────────────────

export type ThoughtStepType = 'observe' | 'plan' | 'act' | 'reflect';

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
}

// ── Stream Token (published to internal:stream:{messageId}) ─────────────────────

export interface StreamToken {
  token: string;
  done: boolean;
  messageId: string;
}

// ── Channel Namespace Prefixes ──────────────────────────────────────────────────

export const CHANNEL_PREFIXES = [
  'internal',
  'intercom',
  'channel',
  'bridge',
  'public',
  'external',
] as const;

export type ChannelPrefix = typeof CHANNEL_PREFIXES[number];
