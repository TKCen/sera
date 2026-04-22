/**
 * Memory-backend capability — mirrors SPEC-memory-pluggability
 * `SemanticMemoryStore`. Plugins that front an external knowledge
 * store (SharePoint, Confluence, a vector DB) implement this.
 */

export interface MemoryRecord {
  id: string;
  content: string;
  embedding?: number[];
  metadata?: Record<string, unknown>;
}

export interface MemoryWriteResult {
  id: string;
}

export interface MemoryQuery {
  query: string;
  limit?: number;
  filter?: Record<string, unknown>;
}

export interface MemoryHit {
  id: string;
  score: number;
  content: string;
  metadata?: Record<string, unknown>;
}

export abstract class MemoryBackend {
  abstract write(record: MemoryRecord): Promise<MemoryWriteResult>;
  abstract search(query: MemoryQuery): Promise<MemoryHit[]>;
  abstract delete(id: string): Promise<void>;
}
