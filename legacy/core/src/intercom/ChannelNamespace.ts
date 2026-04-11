/**
 * ChannelNamespace — utilities for building and validating Centrifugo
 * channel names according to SERA's canonical v1 contract.
 *
 * Channel patterns:
 *   thoughts:{agentId}
 *   tokens:{agentId}
 *   agent:{agentId}:status
 *   private:{agentId}:{targetId}
 *   circle:{circleId}
 *   system.{event}
 */

import { CHANNEL_PREFIXES, type ChannelPrefix } from './types.js';

// Allowed characters in a segment: lowercase alphanumeric, hyphens, underscores
const SEGMENT_RE = /^[a-z0-9][a-z0-9_-]*$/;

export class ChannelNamespace {
  // ── Builders ────────────────────────────────────────────────────────────────

  /** Agent thought-stream channel (UI observability). */
  static thoughts(agentId: string): string {
    return `thoughts:${agentId}`;
  }

  /** Agent lifecycle status channel. */
  static status(agentId: string): string {
    return `agent:${agentId}:status`;
  }

  /**
   * Direct-message channel between two agents.
   * Format: private:{senderId}:{receiverId}
   */
  static private(fromAgentId: string, toAgentId: string): string {
    return `private:${fromAgentId}:${toAgentId}`;
  }

  /** Circle broadcast channel. */
  static circle(circleId: string): string {
    return `circle:${circleId}`;
  }

  /** Platform event channel. */
  static system(event: string): string {
    return `system.${event}`;
  }

  /** LLM token stream channel. */
  static tokens(agentId: string): string {
    return `tokens:${agentId}`;
  }

  /**
   * Bridge channel for cross-instance communication.
   * bridge:dm:{circleA}:{circleB}:{agentA}:{agentB}
   */
  static bridgeDm(
    fromCircle: string,
    toCircle: string,
    fromAgent: string,
    toAgent: string
  ): string {
    return `bridge:dm:${fromCircle}:${toCircle}:${fromAgent}:${toAgent}`;
  }

  // ── Validation ──────────────────────────────────────────────────────────────

  /**
   * Validate that a channel string matches a known namespace pattern.
   * Returns the detected prefix or `null` if invalid.
   */
  static validate(channel: string): ChannelPrefix | null {
    // Platform events use dot separator
    if (channel.startsWith('system.')) {
      const parts = channel.split('.');
      if (parts.length === 2 && SEGMENT_RE.test(parts[1]!)) {
        return 'system';
      }
      return null;
    }

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
      case 'thoughts':
        // thoughts:{agentId}
        return parts.length === 2 ? prefix : null;

      case 'tokens':
        // tokens:{agentId}
        return parts.length === 2 ? prefix : null;

      case 'agent':
        // agent:{agentId}:status
        return parts.length === 3 && parts[2] === 'status' ? prefix : null;

      case 'private':
        // private:{agentId}:{targetId}
        return parts.length === 3 ? prefix : null;

      case 'circle':
        // circle:{circleId}
        return parts.length === 2 ? prefix : null;

      case 'bridge':
        // bridge:dm:{circleA}:{circleB}:{agentA}:{agentB}
        return parts.length >= 2 ? prefix : null;

      case 'federation':
        // federation:{remoteInstance}
        return parts.length === 2 ? prefix : null;

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
    if (channel.startsWith('system.')) return 'system';
    return channel.split(':')[0];
  }
}
