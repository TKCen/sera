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

export type LinkRelationship =
  | 'relatedTo'
  | 'expandsOn'
  | 'contradicts'
  | 'supersedes'
  | 'references'
  | 'derivedFrom';

export const LINK_RELATIONSHIPS: readonly LinkRelationship[] = [
  'relatedTo',
  'expandsOn',
  'contradicts',
  'supersedes',
  'references',
  'derivedFrom',
] as const;

export interface KnowledgeLink {
  target: string;
  relationship: LinkRelationship;
}

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

// ── Link extraction from markdown content ────────────────────────────────

/** Wiki-link pattern: [[target-id]] or [[target-id|relationship]] */
const WIKI_LINK_RE = /\[\[([a-f0-9-]+)(?:\|(\w+))?\]\]/g;

/** Extract links from markdown content using wiki-link syntax. */
export function extractLinks(content: string): KnowledgeLink[] {
  const links: KnowledgeLink[] = [];
  let match: RegExpExecArray | null;
  while ((match = WIKI_LINK_RE.exec(content)) !== null) {
    const target = match[1]!;
    const rel = match[2] as LinkRelationship | undefined;
    links.push({
      target,
      relationship: rel && LINK_RELATIONSHIPS.includes(rel) ? rel : 'relatedTo',
    });
  }
  return links;
}

/** Format a wiki-link for insertion into markdown content. */
export function formatLink(targetId: string, relationship?: LinkRelationship): string {
  if (relationship && relationship !== 'relatedTo') {
    return `[[${targetId}|${relationship}]]`;
  }
  return `[[${targetId}]]`;
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
  excludeTags?: string[];
  since?: string;
  before?: string;
  minImportance?: number;
}
