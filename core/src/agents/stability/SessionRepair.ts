/**
 * SessionRepair — validates and cleans message history before the agent loop.
 *
 * Fixes common corruption patterns:
 * 1. Orphaned tool messages (tool result without a preceding assistant tool_calls)
 * 2. Empty or whitespace-only messages
 * 3. Consecutive same-role messages (merged into one)
 *
 * Returns the cleaned message array and a report of what was fixed.
 */

import type { ChatMessage } from '../types.js';
import { Logger } from '../../lib/logger.js';

const logger = new Logger('SessionRepair');

export interface RepairReport {
  /** Number of orphaned tool messages removed. */
  orphanedToolMessages: number;
  /** Number of empty messages removed. */
  emptyMessages: number;
  /** Number of consecutive same-role merges performed. */
  mergedMessages: number;
  /** True if any repairs were made. */
  repaired: boolean;
}

export class SessionRepair {
  /**
   * Validate and clean a message history array.
   * Returns a new array (does not mutate the input).
   */
  static repair(messages: ChatMessage[]): { messages: ChatMessage[]; report: RepairReport } {
    const report: RepairReport = {
      orphanedToolMessages: 0,
      emptyMessages: 0,
      mergedMessages: 0,
      repaired: false,
    };

    let result = [...messages];

    // Pass 1: Remove empty messages
    const beforeEmpty = result.length;
    result = result.filter((msg) => {
      if (msg.role === 'tool') return true; // tool messages checked in pass 2
      let content = '';
      if (typeof msg.content === 'string') {
        content = msg.content;
      } else if (Array.isArray(msg.content)) {
        content = msg.content
          .map((c) => (c.type === 'text' ? c.text : ''))
          .join('')
          .trim();
      }

      // Allow assistant messages with tool_calls even if content is empty
      if (msg.role === 'assistant' && msg.tool_calls && msg.tool_calls.length > 0) {
        return true;
      }
      return content.trim().length > 0;
    });
    report.emptyMessages = beforeEmpty - result.length;

    // Pass 2: Remove orphaned tool messages
    const beforeOrphans = result.length;
    result = SessionRepair.removeOrphanedToolMessages(result);
    report.orphanedToolMessages = beforeOrphans - result.length;

    // Pass 3: Merge consecutive same-role messages (except tool and system)
    const { merged, mergeCount } = SessionRepair.mergeConsecutive(result);
    result = merged;
    report.mergedMessages = mergeCount;

    report.repaired =
      report.orphanedToolMessages > 0 || report.emptyMessages > 0 || report.mergedMessages > 0;

    if (report.repaired) {
      logger.info(
        `Session repaired: ${report.orphanedToolMessages} orphaned tool msgs removed, ` +
          `${report.emptyMessages} empty msgs removed, ` +
          `${report.mergedMessages} consecutive msgs merged`
      );
    }

    return { messages: result, report };
  }

  // ── Helpers ─────────────────────────────────────────────────────────────────

  /**
   * Remove tool messages that don't have a preceding assistant message
   * with a matching tool_calls entry.
   */
  private static removeOrphanedToolMessages(messages: ChatMessage[]): ChatMessage[] {
    // Collect all tool_call IDs from assistant messages
    const validToolCallIds = new Set<string>();
    for (const msg of messages) {
      if (msg.role === 'assistant' && msg.tool_calls) {
        for (const tc of msg.tool_calls) {
          validToolCallIds.add(tc.id);
        }
      }
    }

    return messages.filter((msg) => {
      if (msg.role !== 'tool') return true;
      const toolCallId = msg.tool_call_id;
      if (!toolCallId || !validToolCallIds.has(toolCallId)) {
        return false; // orphaned
      }
      return true;
    });
  }

  /**
   * Merge consecutive messages with the same role into one.
   * Does NOT merge tool or system messages.
   */
  private static mergeConsecutive(messages: ChatMessage[]): {
    merged: ChatMessage[];
    mergeCount: number;
  } {
    if (messages.length === 0) return { merged: [], mergeCount: 0 };

    const merged: ChatMessage[] = [messages[0]!];
    let mergeCount = 0;

    for (let i = 1; i < messages.length; i++) {
      const current = messages[i]!;
      const prev = merged[merged.length - 1]!;

      // Only merge user or assistant (without tool_calls) messages
      const canMerge =
        current.role === prev.role &&
        current.role !== 'tool' &&
        current.role !== 'system' &&
        !current.tool_calls?.length &&
        !prev.tool_calls?.length;

      if (canMerge) {
        // Merge content
        let newContent: any;
        if (typeof prev.content === 'string' && typeof current.content === 'string') {
          newContent = `${prev.content}\n\n${current.content}`;
        } else {
          // If either is multi-part, normalize both to arrays and concat
          const part1 = typeof prev.content === 'string' ? [{ type: 'text', text: prev.content }] : prev.content;
          const part2 = typeof current.content === 'string' ? [{ type: 'text', text: current.content }] : current.content;
          newContent = [...(part1 as any[]), ...(part2 as any[])];
        }

        merged[merged.length - 1] = {
          ...prev,
          content: newContent,
        };
        mergeCount++;
      } else {
        merged.push(current);
      }
    }

    return { merged, mergeCount };
  }
}
