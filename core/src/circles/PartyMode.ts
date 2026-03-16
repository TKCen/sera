/**
 * PartyMode — multi-agent discussion within a circle.
 *
 * Enables a "party" where 2-3 agents from a circle respond to user
 * messages in character, building on each other's responses.
 */

import { v4 as uuidv4 } from 'uuid';
import type { BaseAgent } from '../agents/BaseAgent.js';
import type { CircleManifest, SelectionStrategy } from './types.js';

// ── Types ───────────────────────────────────────────────────────────────────────

export interface PartyMessage {
  id: string;
  timestamp: string;
  role: 'user' | 'agent';
  agentName?: string;
  agentDisplayName?: string;
  content: string;
}

export interface PartySessionInfo {
  sessionId: string;
  circleId: string;
  agents: string[];
  messageCount: number;
  createdAt: string;
  active: boolean;
}

// ── Exit triggers ───────────────────────────────────────────────────────────────

const EXIT_KEYWORDS = ['exit', 'done', 'end', 'stop', 'quit', 'bye'];
const DEFAULT_MAX_ROUNDS = 20;
const DEFAULT_AGENTS_PER_TURN = 3;

// ── PartySession ────────────────────────────────────────────────────────────────

export class PartySession {
  readonly sessionId: string;
  readonly circleId: string;
  readonly createdAt: string;

  private agents: Map<string, BaseAgent>;
  private agentNames: string[];
  private selectionStrategy: SelectionStrategy;
  private messages: PartyMessage[] = [];
  private roundCount = 0;
  private maxRounds: number;
  private active = true;
  private roundRobinIndex = 0;
  private orchestratorAgent: BaseAgent | undefined;

  constructor(
    circleId: string,
    agents: Map<string, BaseAgent>,
    config?: {
      selectionStrategy?: SelectionStrategy;
      maxRounds?: number;
      orchestratorAgent?: BaseAgent;
    },
  ) {
    this.sessionId = uuidv4();
    this.circleId = circleId;
    this.createdAt = new Date().toISOString();
    this.agents = agents;
    this.agentNames = Array.from(agents.keys());
    this.selectionStrategy = config?.selectionStrategy ?? 'all';
    this.maxRounds = config?.maxRounds ?? DEFAULT_MAX_ROUNDS;
    this.orchestratorAgent = config?.orchestratorAgent;
  }

  /**
   * Send a user message to the party and get agent responses.
   */
  async sendMessage(userMessage: string): Promise<PartyMessage[]> {
    if (!this.active) {
      return [{
        id: uuidv4(),
        timestamp: new Date().toISOString(),
        role: 'agent',
        agentName: 'system',
        agentDisplayName: 'System',
        content: 'This party session has ended.',
      }];
    }

    // Check exit trigger
    if (EXIT_KEYWORDS.includes(userMessage.trim().toLowerCase())) {
      return this.end();
    }

    // Check max rounds
    if (this.roundCount >= this.maxRounds) {
      return this.end();
    }

    // Record user message
    this.messages.push({
      id: uuidv4(),
      timestamp: new Date().toISOString(),
      role: 'user',
      content: userMessage,
    });

    // Select agents for this turn
    const selectedAgents = await this.selectAgents(userMessage);
    const responses: PartyMessage[] = [];

    // Build conversation context
    let priorResponses = '';

    for (const agent of selectedAgents) {
      const contextParts: string[] = [
        `You are in a group discussion with other agents. Respond in character as ${agent.name}.`,
      ];

      if (this.messages.length > 1) {
        const recentHistory = this.messages
          .slice(-10) // last 10 messages for context
          .map(m => {
            if (m.role === 'user') return `User: ${m.content}`;
            return `${m.agentDisplayName ?? m.agentName}: ${m.content}`;
          })
          .join('\n');
        contextParts.push(`Recent discussion:\n${recentHistory}`);
      }

      if (priorResponses) {
        contextParts.push(`Other agents have already responded this turn:\n${priorResponses}`);
      }

      contextParts.push(`User's latest message: ${userMessage}`);
      contextParts.push('Respond concisely and in character. Build on what others have said if relevant.');

      const prompt = contextParts.join('\n\n');

      try {
        const response = await agent.process(prompt);
        const content = response.finalAnswer || response.thought || '';

        const msg: PartyMessage = {
          id: uuidv4(),
          timestamp: new Date().toISOString(),
          role: 'agent',
          agentName: agent.role,
          agentDisplayName: agent.name,
          content,
        };

        responses.push(msg);
        this.messages.push(msg);
        priorResponses += `${agent.name}: ${content}\n\n`;
      } catch (err) {
        console.error(`[PartyMode] Agent "${agent.role}" failed:`, err);
      }
    }

    this.roundCount++;
    return responses;
  }

  /**
   * End the party session and produce a conclusion summary.
   */
  async end(): Promise<PartyMessage[]> {
    this.active = false;

    const summaryMsg: PartyMessage = {
      id: uuidv4(),
      timestamp: new Date().toISOString(),
      role: 'agent',
      agentName: 'system',
      agentDisplayName: 'System',
      content: `Party session ended after ${this.roundCount} round(s) with ${this.agentNames.length} agent(s).`,
    };

    this.messages.push(summaryMsg);
    return [summaryMsg];
  }

  /**
   * Get session info for API responses.
   */
  getInfo(): PartySessionInfo {
    return {
      sessionId: this.sessionId,
      circleId: this.circleId,
      agents: this.agentNames,
      messageCount: this.messages.length,
      createdAt: this.createdAt,
      active: this.active,
    };
  }

  /**
   * Get complete message history.
   */
  getHistory(): PartyMessage[] {
    return [...this.messages];
  }

  isActive(): boolean {
    return this.active;
  }

  // ── Agent Selection ─────────────────────────────────────────────────────────

  private async selectAgents(userMessage: string): Promise<BaseAgent[]> {
    switch (this.selectionStrategy) {
      case 'round-robin':
        return this.selectRoundRobin();
      case 'relevance':
        return this.selectByRelevance(userMessage);
      case 'all':
      default:
        return Array.from(this.agents.values());
    }
  }

  private selectRoundRobin(): BaseAgent[] {
    const count = Math.min(DEFAULT_AGENTS_PER_TURN, this.agentNames.length);
    const selected: BaseAgent[] = [];

    for (let i = 0; i < count; i++) {
      const idx = (this.roundRobinIndex + i) % this.agentNames.length;
      const agent = this.agents.get(this.agentNames[idx]!);
      if (agent) selected.push(agent);
    }

    this.roundRobinIndex = (this.roundRobinIndex + count) % this.agentNames.length;
    return selected;
  }

  private async selectByRelevance(userMessage: string): Promise<BaseAgent[]> {
    // If we have an orchestrator agent, ask it to select
    if (this.orchestratorAgent) {
      try {
        const agentList = this.agentNames.join(', ');
        const response = await this.orchestratorAgent.process(
          `Given the user message: "${userMessage}"\n\n` +
          `Select 2-3 of the following agents that are most relevant to respond:\n${agentList}\n\n` +
          `Respond with ONLY the agent names, comma-separated. No explanation.`,
        );

        const text = response.finalAnswer || response.thought || '';
        const selected: BaseAgent[] = [];

        for (const name of this.agentNames) {
          if (text.toLowerCase().includes(name.toLowerCase())) {
            const agent = this.agents.get(name);
            if (agent) selected.push(agent);
          }
        }

        if (selected.length > 0) return selected;
      } catch {
        // Fall through to fallback
      }
    }

    // Fallback: return up to DEFAULT_AGENTS_PER_TURN agents
    const all = Array.from(this.agents.values());
    return all.slice(0, DEFAULT_AGENTS_PER_TURN);
  }
}

// ── Party Session Manager ───────────────────────────────────────────────────────

export class PartySessionManager {
  private sessions: Map<string, PartySession> = new Map();

  /**
   * Create a new party session for a circle.
   */
  createSession(
    circle: CircleManifest,
    agents: Map<string, BaseAgent>,
    orchestratorAgent?: BaseAgent,
  ): PartySession {
    if (!circle.partyMode?.enabled) {
      throw new Error(
        `Party mode is not enabled for circle "${circle.metadata.name}". ` +
        `Set partyMode.enabled: true in the CIRCLE.yaml.`,
      );
    }

    // Filter to only agents in this circle
    const circleAgents = new Map<string, BaseAgent>();
    for (const agentName of circle.agents) {
      const agent = agents.get(agentName);
      if (agent) circleAgents.set(agentName, agent);
    }

    if (circleAgents.size === 0) {
      throw new Error(
        `No agents available for circle "${circle.metadata.name}"`,
      );
    }

    const sessionConfig: {
      selectionStrategy?: SelectionStrategy;
      maxRounds?: number;
      orchestratorAgent?: BaseAgent;
    } = {};
    if (circle.partyMode.selectionStrategy !== undefined) {
      sessionConfig.selectionStrategy = circle.partyMode.selectionStrategy;
    }
    if (orchestratorAgent !== undefined) {
      sessionConfig.orchestratorAgent = orchestratorAgent;
    }

    const session = new PartySession(circle.metadata.name, circleAgents, sessionConfig);

    this.sessions.set(session.sessionId, session);
    console.log(
      `[PartyMode] Created session ${session.sessionId} for circle "${circle.metadata.name}" ` +
      `with ${circleAgents.size} agents (strategy: ${circle.partyMode.selectionStrategy ?? 'all'})`,
    );

    return session;
  }

  getSession(sessionId: string): PartySession | undefined {
    return this.sessions.get(sessionId);
  }

  endSession(sessionId: string): boolean {
    const session = this.sessions.get(sessionId);
    if (!session) return false;
    session.end();
    this.sessions.delete(sessionId);
    return true;
  }

  listSessions(circleId?: string): PartySessionInfo[] {
    const sessions = Array.from(this.sessions.values());
    const filtered = circleId
      ? sessions.filter(s => s.circleId === circleId)
      : sessions;
    return filtered.map(s => s.getInfo());
  }
}
