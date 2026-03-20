export type ChannelSeverity = 'info' | 'warning' | 'critical';

export type RequestType = 'permission' | 'delegation' | 'knowledge-merge';

export interface ActionPayload {
  requestId: string;
  requestType: RequestType;
  approveToken: string;
  denyToken: string;
  expiresAt: string;
}

export interface ChannelEvent {
  /** Deduplication key. */
  id: string;
  eventType: string;
  title: string;
  body: string;
  severity: ChannelSeverity;
  /** Present if a HitL decision can be made directly from the channel. */
  actions?: ActionPayload;
  metadata: Record<string, unknown>;
  timestamp: string;
}

export type ReplyHandler = (
  requestId: string,
  decision: 'approve' | 'deny',
  platformUserId: string,
) => Promise<void>;

export interface ChannelHealth {
  healthy: boolean;
  latencyMs?: number;
  error?: string;
}

export interface Channel {
  readonly id: string;
  readonly type: string;
  readonly name: string;
  send(event: ChannelEvent): Promise<void>;
  /** Only present on reply-capable channels (Discord bot, Slack app). */
  onReply?(handler: ReplyHandler): void;
  healthCheck(): Promise<ChannelHealth>;
}
