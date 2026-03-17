/**
 * ChannelNamespace — utilities for building and validating Centrifugo
 * channel names according to SERA's namespace scheme.
 *
 * Channel patterns:
 *   internal:agent:{id}:thoughts
 *   internal:agent:{id}:terminal
 *   internal:stream:{messageId}       (per-message token stream)
 *   intercom:{circle}:{from}:{to}    (DM — agent names sorted alphabetically)
 *   channel:{circle}:{name}          (circle pub/sub)
 *   bridge:{circleA}:{circleB}:{name}
 *   public:status:{id}
 *   external:{subscriberId}:inbox
 */

import { CHANNEL_PREFIXES, type ChannelPrefix } from './types.js';

// Allowed characters in a segment: lowercase alphanumeric, hyphens, underscores
const SEGMENT_RE = /^[a-z0-9][a-z0-9_-]*$/;

export class ChannelNamespace {
  // ── Builders ────────────────────────────────────────────────────────────────

  /** Agent thought-stream channel (UI observability). */
  static thoughts(agentId: string): string {
    return `internal:agent:${agentId}:thoughts`;
  }

  /** Agent terminal output channel. */
  static terminal(agentId: string): string {
    return `internal:agent:${agentId}:terminal`;
  }

  /**
   * Direct-message channel between two agents within a circle.
   * Agent names are sorted so both sides derive the same channel.
   */
  static dm(circle: string, agentA: string, agentB: string): string {
    const sorted = [agentA, agentB].sort();
    return `intercom:${circle}:${sorted[0]}:${sorted[1]}`;
  }

  /** Circle-scoped pub/sub channel. */
  static circleChannel(circle: string, channelName: string): string {
    return `channel:${circle}:${channelName}`;
  }

  /** Cross-circle bridge channel (circle names sorted). */
  static bridge(circleA: string, circleB: string, channelName: string): string {
    const sorted = [circleA, circleB].sort();
    return `bridge:${sorted[0]}:${sorted[1]}:${channelName}`;
  }

  /** Public status channel for external subscribers. */
  static status(agentId: string): string {
    return `public:status:${agentId}`;
  }

  /** Per-message streaming channel for token-by-token delivery to the UI. */
  static stream(messageId: string): string {
    return `internal:stream:${messageId}`;
  }

  /** External subscriber inbox. */
  static externalInbox(subscriberId: string): string {
    return `external:${subscriberId}:inbox`;
  }

  // ── Validation ──────────────────────────────────────────────────────────────

  /**
   * Validate that a channel string matches a known namespace pattern.
   * Returns the detected prefix or `null` if invalid.
   */
  static validate(channel: string): ChannelPrefix | null {
    const parts = channel.split(':');
    if (parts.length < 2) return null;

    const prefix = parts[0] as ChannelPrefix;
    if (!(CHANNEL_PREFIXES as readonly string[]).includes(prefix)) return null;

    // All segments must be non-empty and use valid characters
    for (const segment of parts) {
      if (!SEGMENT_RE.test(segment)) return null;
    }

    // Validate structure per prefix
    switch (prefix) {
      case 'internal':
        // internal:agent:{id}:thoughts | internal:agent:{id}:terminal
        if (parts.length === 4 && parts[1] === 'agent'
          && (parts[3] === 'thoughts' || parts[3] === 'terminal')) return prefix;
        // internal:stream:{messageId}
        if (parts.length === 3 && parts[1] === 'stream') return prefix;
        return null;

      case 'intercom':
        // intercom:{circle}:{from}:{to}
        return parts.length === 4 ? prefix : null;

      case 'channel':
        // channel:{circle}:{name}
        return parts.length === 3 ? prefix : null;

      case 'bridge':
        // bridge:{circleA}:{circleB}:{name}
        return parts.length === 4 ? prefix : null;

      case 'public':
        // public:status:{id}
        return parts.length === 3 && parts[1] === 'status' ? prefix : null;

      case 'external':
        // external:{subscriberId}:inbox
        return parts.length === 3 && parts[2] === 'inbox' ? prefix : null;

      default:
        return null;
    }
  }

  /** Returns true if the channel string is valid. */
  static isValid(channel: string): boolean {
    return ChannelNamespace.validate(channel) !== null;
  }

  /** Extract the prefix namespace from a channel string. */
  static getPrefix(channel: string): string | undefined {
    return channel.split(':')[0];
  }
}
