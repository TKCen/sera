/**
 * Memory Blocks — Letta-style × Obsidian-style hybrid memory system.
 *
 * Each memory entry is stored as a markdown file with YAML frontmatter.
 * Entries link to each other via explicit `refs` and implicit `[[Title]]` wikilinks.
 */

// ── Block Types ─────────────────────────────────────────────────────────────────

/** The four Letta-style memory block categories. */
export type MemoryBlockType = 'human' | 'persona' | 'core' | 'archive';

export const MEMORY_BLOCK_TYPES: readonly MemoryBlockType[] = [
  'human',
  'persona',
  'core',
  'archive',
] as const;

// ── Entry ───────────────────────────────────────────────────────────────────────

/** Provenance of a memory entry. */
export type MemorySource = 'user' | 'agent' | 'reflector' | 'system';

/** A single memory entry, corresponding to one `.md` file on disk. */
export interface MemoryEntry {
  /** Stable UUID for addressing. */
  id: string;
  /** Human-readable title (also used for the filename slug). */
  title: string;
  /** Which block this entry belongs to. */
  type: MemoryBlockType;
  /** The markdown body content. */
  content: string;
  /** Explicit refs to other entries by ID (graph edges). */
  refs: string[];
  /** Searchable tags. */
  tags: string[];
  /** How this entry was created. */
  source: MemorySource;
  /** ISO-8601 timestamp. */
  createdAt: string;
  /** ISO-8601 timestamp. */
  updatedAt: string;
  /** Importance 1-5. */
  importance?: number;
}

/** Options for creating a new memory entry. */
export interface CreateEntryOptions {
  title: string;
  content: string;
  refs?: string[];
  tags?: string[];
  source?: MemorySource;
  importance?: number;
}

// ── Block ───────────────────────────────────────────────────────────────────────

/** A memory block = a collection of entries of the same type. */
export interface MemoryBlock {
  type: MemoryBlockType;
  entries: MemoryEntry[];
}

// ── Graph ───────────────────────────────────────────────────────────────────────

export interface GraphNode {
  id: string;
  title: string;
  type: MemoryBlockType;
  tags: string[];
}

export interface GraphEdge {
  from: string;
  to: string;
  /** 'ref' = explicit frontmatter ref, 'wikilink' = parsed [[Title]] link. */
  kind: 'ref' | 'wikilink';
}

/** Full graph structure for visualization (e.g. Obsidian-style). */
export interface MemoryGraph {
  nodes: GraphNode[];
  edges: GraphEdge[];
}
