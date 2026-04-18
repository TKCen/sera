import { describe, it, expect } from 'vitest';
import { SessionRepair } from './SessionRepair.js';
import type { ChatMessage } from '../types.js';

// ── Helpers ─────────────────────────────────────────────────────────────────────

function msg(role: string, content: string, extra?: Partial<ChatMessage>): ChatMessage {
  return { role, content, ...extra } as ChatMessage;
}

function toolMsg(content: string, toolCallId: string): ChatMessage {
  return { role: 'tool', content, tool_call_id: toolCallId } as ChatMessage;
}

function assistantWithTools(content: string, toolCallIds: string[]): ChatMessage {
  return {
    role: 'assistant',
    content,
    tool_calls: toolCallIds.map((id) => ({
      id,
      type: 'function' as const,
      function: { name: 'test', arguments: '{}' },
    })),
  } as ChatMessage;
}

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('SessionRepair', () => {
  // ── Clean history passthrough ─────────────────────────────────────────────

  it('should pass through a clean history unchanged', () => {
    const history = [msg('user', 'Hello'), msg('assistant', 'Hi there!')];

    const { messages, report } = SessionRepair.repair(history);

    expect(messages).toEqual(history);
    expect(report.repaired).toBe(false);
  });

  // ── Empty message removal ─────────────────────────────────────────────────

  it('should remove empty messages', () => {
    const history = [
      msg('user', 'Hello'),
      msg('assistant', ''),
      msg('user', '   '),
      msg('assistant', 'Response'),
    ];

    const { messages, report } = SessionRepair.repair(history);

    expect(messages).toHaveLength(2);
    expect(messages[0]!.content).toBe('Hello');
    expect(messages[1]!.content).toBe('Response');
    expect(report.emptyMessages).toBe(2);
    expect(report.repaired).toBe(true);
  });

  it('should keep assistant messages with tool_calls even if content is empty', () => {
    const history = [
      msg('user', 'Search for something'),
      assistantWithTools('', ['tc-1']),
      toolMsg('result data', 'tc-1'),
      msg('assistant', 'Here is what I found.'),
    ];

    const { messages, report } = SessionRepair.repair(history);

    expect(messages).toHaveLength(4);
    expect(report.emptyMessages).toBe(0);
  });

  // ── Orphaned tool message removal ─────────────────────────────────────────

  it('should remove orphaned tool messages', () => {
    const history = [
      msg('user', 'Hello'),
      toolMsg('orphaned result', 'tc-nonexistent'),
      msg('assistant', 'Regular response'),
    ];

    const { messages, report } = SessionRepair.repair(history);

    expect(messages).toHaveLength(2);
    expect(messages.find((m) => m.role === 'tool')).toBeUndefined();
    expect(report.orphanedToolMessages).toBe(1);
    expect(report.repaired).toBe(true);
  });

  it('should keep tool messages with valid tool_call_id', () => {
    const history = [
      msg('user', 'Search'),
      assistantWithTools('', ['tc-1']),
      toolMsg('result', 'tc-1'),
      msg('assistant', 'Done'),
    ];

    const { messages, report } = SessionRepair.repair(history);

    expect(messages).toHaveLength(4);
    expect(report.orphanedToolMessages).toBe(0);
  });

  // ── Consecutive same-role merge ───────────────────────────────────────────

  it('should merge consecutive user messages', () => {
    const history = [
      msg('user', 'First part'),
      msg('user', 'Second part'),
      msg('assistant', 'Response'),
    ];

    const { messages, report } = SessionRepair.repair(history);

    expect(messages).toHaveLength(2);
    expect(messages[0]!.content).toBe('First part\n\nSecond part');
    expect(report.mergedMessages).toBe(1);
    expect(report.repaired).toBe(true);
  });

  it('should merge consecutive assistant messages', () => {
    const history = [
      msg('user', 'Question'),
      msg('assistant', 'Part 1'),
      msg('assistant', 'Part 2'),
      msg('assistant', 'Part 3'),
    ];

    const { messages, report } = SessionRepair.repair(history);

    expect(messages).toHaveLength(2);
    expect(messages[1]!.content).toBe('Part 1\n\nPart 2\n\nPart 3');
    expect(report.mergedMessages).toBe(2);
  });

  it('should NOT merge tool or system messages', () => {
    const history = [msg('system', 'Prompt A'), msg('system', 'Prompt B'), msg('user', 'Hello')];

    const { messages, report } = SessionRepair.repair(history);

    // System messages should NOT be merged
    expect(messages).toHaveLength(3);
    expect(report.mergedMessages).toBe(0);
  });

  it('should NOT merge assistant messages that have tool_calls', () => {
    const history = [
      msg('user', 'Search'),
      assistantWithTools('Searching...', ['tc-1']),
      toolMsg('results', 'tc-1'),
      msg('assistant', 'Summary'),
    ];

    const { messages, report } = SessionRepair.repair(history);

    // The assistant + tool + assistant sequence should not merge
    expect(messages).toHaveLength(4);
    expect(report.mergedMessages).toBe(0);
  });

  // ── Combined scenarios ────────────────────────────────────────────────────

  it('should handle all repair types simultaneously', () => {
    const history = [
      msg('user', 'Hello'),
      msg('user', 'World'), // will be merged with previous
      msg('assistant', ''), // will be removed (empty)
      toolMsg('orphan', 'tc-bad'), // will be removed (orphaned)
      msg('assistant', 'Response'),
    ];

    const { messages, report } = SessionRepair.repair(history);

    expect(messages).toHaveLength(2);
    expect(messages[0]!.content).toBe('Hello\n\nWorld');
    expect(messages[1]!.content).toBe('Response');
    expect(report.emptyMessages).toBe(1);
    expect(report.orphanedToolMessages).toBe(1);
    expect(report.mergedMessages).toBe(1);
    expect(report.repaired).toBe(true);
  });

  it('should return empty array for empty input', () => {
    const { messages, report } = SessionRepair.repair([]);

    expect(messages).toEqual([]);
    expect(report.repaired).toBe(false);
  });
});
