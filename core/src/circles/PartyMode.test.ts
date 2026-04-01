import { describe, it, expect, vi, beforeEach } from 'vitest';
import { PartySession, PartySessionManager } from './PartyMode.js';
import type { CircleManifest } from './types.js';
import type { AgentResponse } from '../agents/index.js';
import type { BaseAgent } from '../agents/index.js';

// ── Mock Agent ──────────────────────────────────────────────────────────────────

function createMockAgent(roleName: string, displayName: string, response: string) {
  return {
    name: displayName,
    role: roleName,
    process: vi.fn().mockResolvedValue({
      thought: `${displayName} thinking`,
      finalAnswer: response,
    } satisfies AgentResponse),
    updateLlmProvider: vi.fn(),
  } as unknown as BaseAgent;
}

// ── Mock Circle ─────────────────────────────────────────────────────────────────

function createMockCircle(overrides?: Partial<CircleManifest>): CircleManifest {
  return {
    apiVersion: 'sera/v1',
    kind: 'Circle',
    metadata: { name: 'test-circle', displayName: 'Test Circle' },
    agents: ['agent-a', 'agent-b'],
    partyMode: { enabled: true, selectionStrategy: 'all' },
    ...overrides,
  };
}

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('PartySession', () => {
  let agents: Map<string, BaseAgent>;

  beforeEach(() => {
    agents = new Map([
      ['agent-a', createMockAgent('agent-a', 'Alice', 'Alice says hello')],
      ['agent-b', createMockAgent('agent-b', 'Bob', 'Bob agrees')],
    ]);
  });

  it('should create a session and return agent responses', async () => {
    const session = new PartySession('test-circle', agents);
    const responses = await session.sendMessage('What do you think?');

    expect(responses.length).toBe(2);
    expect(responses[0]!.role).toBe('agent');
    expect(responses[0]!.agentName).toBe('agent-a');
    expect(responses[0]!.content).toBe('Alice says hello');
    expect(responses[1]!.agentName).toBe('agent-b');
    expect(session.isActive()).toBe(true);
  });

  it('should end on exit keyword', async () => {
    const session = new PartySession('test-circle', agents);
    const responses = await session.sendMessage('exit');

    expect(responses.length).toBe(1);
    expect(responses[0]!.agentName).toBe('system');
    expect(session.isActive()).toBe(false);
  });

  it('should include prior responses as context for later agents', async () => {
    const session = new PartySession('test-circle', agents);
    await session.sendMessage('Discuss testing');

    // Second agent should have seen first agent's response in its prompt
    const agentB = agents.get('agent-b') as unknown as { process: import('vitest').Mock };
    const prompt = agentB.process.mock.calls[0]![0] as string;
    expect(prompt).toContain('Alice says hello');
  });

  it('should return ended message when session is closed', async () => {
    const session = new PartySession('test-circle', agents);
    await session.end();

    const responses = await session.sendMessage('Hello?');
    expect(responses[0]!.content).toContain('ended');
  });

  it('should use round-robin selection', async () => {
    const threeAgents = new Map([
      ['a', createMockAgent('a', 'A', 'A')],
      ['b', createMockAgent('b', 'B', 'B')],
      ['c', createMockAgent('c', 'C', 'C')],
      ['d', createMockAgent('d', 'D', 'D')],
    ]);

    const session = new PartySession('test', threeAgents, {
      selectionStrategy: 'round-robin',
    });

    const r1 = await session.sendMessage('Round 1');
    // Should select first 3 agents (a, b, c)
    expect(r1.length).toBe(3);

    const r2 = await session.sendMessage('Round 2');
    // Should wrap around (d, a, b)
    expect(r2.length).toBe(3);
  });

  it('should provide session info', () => {
    const session = new PartySession('test-circle', agents);
    const info = session.getInfo();

    expect(info.circleId).toBe('test-circle');
    expect(info.agents).toEqual(['agent-a', 'agent-b']);
    expect(info.active).toBe(true);
    expect(info.messageCount).toBe(0);
  });
});

describe('PartySessionManager', () => {
  it('should create and retrieve sessions', () => {
    const manager = new PartySessionManager();
    const circle = createMockCircle();
    const agents = new Map([
      ['agent-a', createMockAgent('agent-a', 'Alice', 'hi')],
      ['agent-b', createMockAgent('agent-b', 'Bob', 'hey')],
    ]);

    const session = manager.createSession(circle, agents);
    expect(session).toBeDefined();
    expect(manager.getSession(session.sessionId)).toBe(session);
  });

  it('should reject when party mode is disabled', () => {
    const manager = new PartySessionManager();
    const circle = createMockCircle({ partyMode: { enabled: false } });
    const agents = new Map([['agent-a', createMockAgent('agent-a', 'A', '')]]);

    expect(() => manager.createSession(circle, agents)).toThrow(/not enabled/);
  });

  it('should reject when no agents are available', () => {
    const manager = new PartySessionManager();
    const circle = createMockCircle();
    const agents = new Map(); // empty

    expect(() => manager.createSession(circle, agents)).toThrow(/No agents available/);
  });

  it('should end and remove sessions', () => {
    const manager = new PartySessionManager();
    const circle = createMockCircle();
    const agents = new Map([
      ['agent-a', createMockAgent('agent-a', 'A', '')],
      ['agent-b', createMockAgent('agent-b', 'B', '')],
    ]);

    const session = manager.createSession(circle, agents);
    expect(manager.endSession(session.sessionId)).toBe(true);
    expect(manager.getSession(session.sessionId)).toBeUndefined();
  });

  it('should list sessions filtered by circle', () => {
    const manager = new PartySessionManager();
    const circle = createMockCircle();
    const agents = new Map([
      ['agent-a', createMockAgent('agent-a', 'A', '')],
      ['agent-b', createMockAgent('agent-b', 'B', '')],
    ]);

    manager.createSession(circle, agents);
    const sessions = manager.listSessions('test-circle');
    expect(sessions).toHaveLength(1);
    expect(sessions[0]!.circleId).toBe('test-circle');
  });
});
