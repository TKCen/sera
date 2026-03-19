/**
 * Epic 8 — Memory block types for the three-scope memory model.
 *
 * These types are distinct from the Letta-style types in types.ts.
 * They match the ARCHITECTURE.md memory block spec.
 */

export type KnowledgeBlockType =
  | 'fact'
  | 'context'
  | 'memory'
  | 'insight'
  | 'reference'
  | 'observation'
  | 'decision';

export const KNOWLEDGE_BLOCK_TYPES: readonly KnowledgeBlockType[] = [
  'fact',
  'context',
  'memory',
  'insight',
  'reference',
  'observation',
  'decision',
] as const;

export type MemoryScope = 'personal' | 'circle' | 'global';

export type Importance = 1 | 2 | 3 | 4 | 5;

export interface KnowledgeBlock {
  id: string;
  agentId: string;
  type: KnowledgeBlockType;
  timestamp: string;
  tags: string[];
  importance: Importance;
  title: string;
  content: string;
  compacted?: boolean;
}

export interface KnowledgeBlockCreateOpts {
  content: string;
  type: KnowledgeBlockType;
  agentId: string;
  tags?: string[];
  importance?: Importance;
  title?: string;
}

export interface KnowledgeBlockListFilters {
  type?: KnowledgeBlockType;
  tags?: string[];
  since?: string;
  before?: string;
}
