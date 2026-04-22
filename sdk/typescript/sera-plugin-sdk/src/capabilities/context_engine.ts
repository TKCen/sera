/**
 * Context-engine capability triad — mirrors
 * `SPEC-context-engine-pluggability` §2 (`ContextEngine`, `ContextQuery`,
 * `ContextDiagnostics`). A plugin that wraps an LCM-style engine
 * typically implements all three, e.g.:
 *
 * ```ts
 * class LcmPlugin implements ContextEngine, ContextQuery, ContextDiagnostics {
 *   async ingest(msg: IngestMessage): Promise<void> { ... }
 *   async assemble(budget: AssembleBudget): Promise<AssembleResult> { ... }
 *   async search(req: SearchRequest): Promise<SearchResult> { ... }
 *   async status(sessionId: string): Promise<EngineStatus> { ... }
 * }
 * ```
 *
 * Declared as abstract classes (rather than bare interfaces) so that a
 * user can also `extends ContextEngine` if they prefer inheritance, and
 * so that SDK-side dispatch can use `instanceof` where convenient.
 */

export interface IngestMessage {
  session_id: string;
  role: "user" | "assistant" | "system" | "tool";
  content: string;
  metadata?: Record<string, unknown>;
}

export interface AssembleBudget {
  session_id: string;
  token_budget: number;
  reserve?: number;
}

export interface AssembleResult {
  messages: Array<{
    role: string;
    content: string;
  }>;
  tokens_used: number;
  truncated: boolean;
}

export interface SearchRequest {
  session_id: string;
  query: string;
  limit?: number;
}

export interface SearchHit {
  id: string;
  score: number;
  content: string;
  metadata?: Record<string, unknown>;
}

export interface SearchResult {
  hits: SearchHit[];
}

export interface EngineStatus {
  session_id: string;
  healthy: boolean;
  compacted_turns: number;
  live_turns: number;
  details?: Record<string, unknown>;
}

export interface DoctorReport {
  healthy: boolean;
  issues: string[];
  remediations: string[];
}

export abstract class ContextEngine {
  abstract ingest(msg: IngestMessage): Promise<void>;
  abstract assemble(budget: AssembleBudget): Promise<AssembleResult>;
}

export abstract class ContextQuery {
  abstract search(req: SearchRequest): Promise<SearchResult>;
  abstract expand(id: string): Promise<SearchHit>;
}

export abstract class ContextDiagnostics {
  abstract status(sessionId: string): Promise<EngineStatus>;
  abstract doctor(): Promise<DoctorReport>;
}
