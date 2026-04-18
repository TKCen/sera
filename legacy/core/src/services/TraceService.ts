/**
 * TraceService — Interaction Trace Persistence (Epic 30, Story 30.1)
 *
 * Accumulates structured reasoning trace data per agent+session and persists
 * full interaction traces to the `interaction_traces` table after each turn.
 *
 * TraceAccumulator is keyed by `agentInstanceId::sessionId` with TTL cleanup.
 */

import { pool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('TraceService');

/** Maximum tokens to include in the auto-generated summary. */
const SUMMARY_MAX_TOKENS = 1000;

/** Accumulator TTL in milliseconds (30 minutes). */
const ACCUMULATOR_TTL_MS = 30 * 60 * 1000;

// ── Types ─────────────────────────────────────────────────────────────────────

export interface TraceMessage {
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  tool_calls?: Array<{
    id: string;
    type: string;
    function: { name: string; arguments: string };
  }>;
  tool_call_id?: string;
  name?: string;
  timestamp?: string;
}

export interface TraceToolUse {
  toolName: string;
  arguments: Record<string, unknown>;
  result: unknown;
  durationMs?: number;
  timestamp?: string;
}

export interface TraceData {
  messages: TraceMessage[];
  toolUses: TraceToolUse[];
  model?: string;
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  startedAt: string;
  completedAt?: string;
}

export interface InteractionTrace {
  id: string;
  agent_instance_id: string;
  session_id: string;
  trace_data: TraceData;
  summary: string | null;
  token_count: number;
  created_at: Date;
  updated_at: Date;
}

interface AccumulatorEntry {
  data: TraceData;
  lastTouchedAt: number;
}

// ── TraceAccumulator ──────────────────────────────────────────────────────────

/**
 * In-memory accumulator keyed by `agentInstanceId::sessionId`.
 * Collects messages and tool uses across a session before final persistence.
 */
export class TraceAccumulator {
  private static instance: TraceAccumulator;
  private entries = new Map<string, AccumulatorEntry>();
  private cleanupTimer: ReturnType<typeof setInterval> | null = null;

  private constructor() {
    // Run TTL cleanup every 5 minutes
    this.cleanupTimer = setInterval(() => this.cleanup(), 5 * 60 * 1000);
  }

  public static getInstance(): TraceAccumulator {
    if (!TraceAccumulator.instance) {
      TraceAccumulator.instance = new TraceAccumulator();
    }
    return TraceAccumulator.instance;
  }

  private key(agentInstanceId: string, sessionId: string): string {
    return `${agentInstanceId}::${sessionId}`;
  }

  public getOrCreate(agentInstanceId: string, sessionId: string): TraceData {
    const k = this.key(agentInstanceId, sessionId);
    let entry = this.entries.get(k);
    if (!entry) {
      entry = {
        data: {
          messages: [],
          toolUses: [],
          promptTokens: 0,
          completionTokens: 0,
          totalTokens: 0,
          startedAt: new Date().toISOString(),
        },
        lastTouchedAt: Date.now(),
      };
      this.entries.set(k, entry);
    } else {
      entry.lastTouchedAt = Date.now();
    }
    return entry.data;
  }

  public addMessage(agentInstanceId: string, sessionId: string, message: TraceMessage): void {
    const trace = this.getOrCreate(agentInstanceId, sessionId);
    trace.messages.push(message);
  }

  public addToolUse(agentInstanceId: string, sessionId: string, toolUse: TraceToolUse): void {
    const trace = this.getOrCreate(agentInstanceId, sessionId);
    trace.toolUses.push(toolUse);
  }

  public recordTokens(
    agentInstanceId: string,
    sessionId: string,
    prompt: number,
    completion: number
  ): void {
    const trace = this.getOrCreate(agentInstanceId, sessionId);
    trace.promptTokens += prompt;
    trace.completionTokens += completion;
    trace.totalTokens += prompt + completion;
  }

  public setModel(agentInstanceId: string, sessionId: string, model: string): void {
    const trace = this.getOrCreate(agentInstanceId, sessionId);
    trace.model = model;
  }

  public get(agentInstanceId: string, sessionId: string): TraceData | undefined {
    return this.entries.get(this.key(agentInstanceId, sessionId))?.data;
  }

  public finalize(agentInstanceId: string, sessionId: string): TraceData | undefined {
    const k = this.key(agentInstanceId, sessionId);
    const entry = this.entries.get(k);
    if (!entry) return undefined;
    entry.data.completedAt = new Date().toISOString();
    this.entries.delete(k);
    return entry.data;
  }

  private cleanup(): void {
    const now = Date.now();
    for (const [k, entry] of this.entries) {
      if (now - entry.lastTouchedAt > ACCUMULATOR_TTL_MS) {
        logger.debug(`TraceAccumulator: evicting stale entry ${k}`);
        this.entries.delete(k);
      }
    }
  }

  public stop(): void {
    if (this.cleanupTimer) {
      clearInterval(this.cleanupTimer);
      this.cleanupTimer = null;
    }
  }

  /** Exposed for testing only. */
  public clear(): void {
    this.entries.clear();
  }
}

// ── TraceService ──────────────────────────────────────────────────────────────

export class TraceService {
  private static instance: TraceService;
  private accumulator = TraceAccumulator.getInstance();

  private constructor() {}

  public static getInstance(): TraceService {
    if (!TraceService.instance) {
      TraceService.instance = new TraceService();
    }
    return TraceService.instance;
  }

  /**
   * Persist a finalized trace for the given agent+session.
   * Called after a complete interaction turn.
   */
  public async persist(
    agentInstanceId: string,
    sessionId: string
  ): Promise<InteractionTrace | null> {
    const traceData = this.accumulator.finalize(agentInstanceId, sessionId);
    if (!traceData) return null;

    const summary = generateSummary(traceData);
    const tokenCount = traceData.totalTokens;

    try {
      const { rows } = await pool.query<InteractionTrace>(
        `INSERT INTO interaction_traces
           (agent_instance_id, session_id, trace_data, summary, token_count)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING *`,
        [agentInstanceId, sessionId, JSON.stringify(traceData), summary, tokenCount]
      );
      const trace = rows[0]!;
      logger.debug(`Persisted trace ${trace.id} for agent=${agentInstanceId} session=${sessionId}`);
      return trace;
    } catch (err) {
      logger.error(
        `Failed to persist trace for agent=${agentInstanceId} session=${sessionId}:`,
        err
      );
      return null;
    }
  }

  /** Accumulate a message into the current session trace. */
  public addMessage(agentInstanceId: string, sessionId: string, message: TraceMessage): void {
    this.accumulator.addMessage(agentInstanceId, sessionId, message);
  }

  /** Accumulate a tool use event into the current session trace. */
  public addToolUse(agentInstanceId: string, sessionId: string, toolUse: TraceToolUse): void {
    this.accumulator.addToolUse(agentInstanceId, sessionId, toolUse);
  }

  /** Record token counts from an LLM response. */
  public recordTokens(
    agentInstanceId: string,
    sessionId: string,
    prompt: number,
    completion: number
  ): void {
    this.accumulator.recordTokens(agentInstanceId, sessionId, prompt, completion);
  }

  /** Set the model used in this session. */
  public setModel(agentInstanceId: string, sessionId: string, model: string): void {
    this.accumulator.setModel(agentInstanceId, sessionId, model);
  }

  // ── Query methods ────────────────────────────────────────────────────────────

  public async listTraces(
    agentInstanceId?: string,
    limit = 50,
    offset = 0
  ): Promise<InteractionTrace[]> {
    if (agentInstanceId) {
      const { rows } = await pool.query<InteractionTrace>(
        `SELECT * FROM interaction_traces
         WHERE agent_instance_id = $1
         ORDER BY created_at DESC
         LIMIT $2 OFFSET $3`,
        [agentInstanceId, limit, offset]
      );
      return rows;
    }
    const { rows } = await pool.query<InteractionTrace>(
      `SELECT * FROM interaction_traces
       ORDER BY created_at DESC
       LIMIT $1 OFFSET $2`,
      [limit, offset]
    );
    return rows;
  }

  public async getTrace(id: string): Promise<InteractionTrace | null> {
    const { rows } = await pool.query<InteractionTrace>(
      'SELECT * FROM interaction_traces WHERE id = $1',
      [id]
    );
    return rows[0] ?? null;
  }

  public async getTracesBySession(
    agentInstanceId: string,
    sessionId: string
  ): Promise<InteractionTrace[]> {
    const { rows } = await pool.query<InteractionTrace>(
      `SELECT * FROM interaction_traces
       WHERE agent_instance_id = $1 AND session_id = $2
       ORDER BY created_at DESC`,
      [agentInstanceId, sessionId]
    );
    return rows;
  }

  public async deleteTrace(id: string): Promise<boolean> {
    const { rowCount } = await pool.query('DELETE FROM interaction_traces WHERE id = $1', [id]);
    return (rowCount ?? 0) > 0;
  }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/**
 * Generate a concise summary from trace data, capped at SUMMARY_MAX_TOKENS
 * (approximated at 4 chars/token).
 */
function generateSummary(trace: TraceData): string {
  const maxChars = SUMMARY_MAX_TOKENS * 4;

  const parts: string[] = [];

  // First user message
  const firstUser = trace.messages.find((m) => m.role === 'user');
  if (firstUser) {
    parts.push(`User: ${firstUser.content.substring(0, 300)}`);
  }

  // Last assistant message
  const assistantMessages = trace.messages.filter((m) => m.role === 'assistant');
  const lastAssistant = assistantMessages[assistantMessages.length - 1];
  if (lastAssistant) {
    parts.push(`Assistant: ${lastAssistant.content.substring(0, 500)}`);
  }

  // Tool uses summary
  if (trace.toolUses.length > 0) {
    const toolNames = [...new Set(trace.toolUses.map((t) => t.toolName))];
    parts.push(`Tools used: ${toolNames.join(', ')}`);
  }

  // Token stats
  parts.push(
    `Tokens: ${trace.promptTokens} prompt + ${trace.completionTokens} completion = ${trace.totalTokens} total`
  );

  const full = parts.join('\n\n');
  return full.length > maxChars ? full.substring(0, maxChars) + '...' : full;
}
