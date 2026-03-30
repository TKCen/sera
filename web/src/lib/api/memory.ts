import { request } from './client';
import type { MemoryBlock, MemoryEntry, MemoryGraph, SearchResult } from './types';

export function getMemoryBlocks(): Promise<MemoryBlock[]> {
  return request<MemoryBlock[]>('/memory/blocks');
}

export function getMemoryBlock(type: string): Promise<MemoryBlock> {
  return request<MemoryBlock>(`/memory/blocks/${encodeURIComponent(type)}`);
}

export function addMemoryEntry(
  type: string,
  entry: Pick<MemoryEntry, 'title' | 'content'> & {
    refs?: string[];
    tags?: string[];
    source?: string;
  }
): Promise<MemoryEntry> {
  return request<MemoryEntry>(`/memory/blocks/${encodeURIComponent(type)}`, {
    method: 'POST',
    body: JSON.stringify(entry),
  });
}

export function getMemoryEntry(id: string): Promise<MemoryEntry> {
  return request<MemoryEntry>(`/memory/entries/${encodeURIComponent(id)}`);
}

export function updateMemoryEntry(id: string, content: string): Promise<MemoryEntry> {
  return request<MemoryEntry>(`/memory/entries/${encodeURIComponent(id)}`, {
    method: 'PUT',
    body: JSON.stringify({ content }),
  });
}

export function deleteMemoryEntry(id: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/memory/entries/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

export function addMemoryRef(id: string, targetId: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/memory/entries/${encodeURIComponent(id)}/refs`, {
    method: 'POST',
    body: JSON.stringify({ targetId }),
  });
}

export function deleteMemoryRef(id: string, targetId: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(
    `/memory/entries/${encodeURIComponent(id)}/refs/${encodeURIComponent(targetId)}`,
    { method: 'DELETE' }
  );
}

export function getMemoryGraph(): Promise<MemoryGraph> {
  return request<MemoryGraph>('/memory/graph');
}

export function searchMemory(query: string, limit?: number): Promise<SearchResult[]> {
  const params = new URLSearchParams({ query });
  if (limit !== undefined) params.set('limit', String(limit));
  return request<SearchResult[]>(`/memory/search?${params.toString()}`);
}

// ── Agent-scoped memory (Epic 8) ─────────────────────────────────────────

export interface ScopedBlock {
  id: string;
  agentId: string;
  type: string;
  timestamp: string;
  tags: string[];
  importance: number;
  title: string;
  content: string;
}

export interface ScopedStats {
  blockCount: number;
  vectorCount: number;
}

export interface GraphData {
  nodes: Array<{ id: string; title: string; type: string }>;
  edges: Array<{ source: string; target: string; relationship: string }>;
}

export function getAgentBlocks(
  agentId: string,
  params?: { tags?: string; excludeTags?: string; type?: string; minImportance?: number }
): Promise<ScopedBlock[]> {
  const q = new URLSearchParams();
  if (params?.tags) q.set('tags', params.tags);
  if (params?.excludeTags) q.set('excludeTags', params.excludeTags);
  if (params?.type) q.set('type', params.type);
  if (params?.minImportance) q.set('minImportance', String(params.minImportance));
  const qs = q.toString();
  return request<ScopedBlock[]>(
    `/memory/${encodeURIComponent(agentId)}/blocks${qs ? `?${qs}` : ''}`
  );
}

export function getAgentBlock(agentId: string, blockId: string): Promise<ScopedBlock> {
  return request<ScopedBlock>(
    `/memory/${encodeURIComponent(agentId)}/blocks/${encodeURIComponent(blockId)}`
  );
}

export function getAgentStats(agentId: string): Promise<ScopedStats> {
  return request<ScopedStats>(`/memory/${encodeURIComponent(agentId)}/stats`);
}

export function getAgentGraph(agentId: string): Promise<GraphData> {
  return request<GraphData>(`/memory/${encodeURIComponent(agentId)}/graph`);
}

export function getAgentLinks(
  agentId: string,
  entryId?: string
): Promise<Array<{ sourceId: string; sourceTitle: string; target: string; relationship: string }>> {
  const qs = entryId ? `?entryId=${encodeURIComponent(entryId)}` : '';
  return request(`/memory/${encodeURIComponent(agentId)}/links${qs}`);
}

export function getAgentBacklinks(
  agentId: string,
  blockId: string
): Promise<
  Array<{ sourceId: string; sourceTitle: string; sourceType: string; relationship: string }>
> {
  return request(
    `/memory/${encodeURIComponent(agentId)}/blocks/${encodeURIComponent(blockId)}/backlinks`
  );
}

export function promoteBlock(
  agentId: string,
  blockId: string,
  targetScope: 'circle' | 'global',
  circleId?: string
): Promise<ScopedBlock> {
  return request(
    `/memory/${encodeURIComponent(agentId)}/blocks/${encodeURIComponent(blockId)}/promote`,
    {
      method: 'POST',
      body: JSON.stringify({ targetScope, ...(circleId ? { circleId } : {}) }),
    }
  );
}

// ── Cross-agent memory (#352) ─────────────────────────────────────────

export interface MemoryOverview {
  totalBlocks: number;
  agents: Array<{ id: string; blockCount: number }>;
  topTags: Array<{ tag: string; count: number }>;
  typeBreakdown: Record<string, number>;
}

export interface ExplorerGraphNode {
  id: string;
  title: string;
  type: string;
  tags: string[];
  nodeKind: 'block' | 'agent' | 'circle';
  agentId?: string;
}

export interface ExplorerGraphEdge {
  source: string;
  target: string;
  kind: string;
  relationship?: string;
}

export interface ExplorerGraphData {
  nodes: ExplorerGraphNode[];
  edges: ExplorerGraphEdge[];
}

export interface MemorySearchResult {
  block: ScopedBlock;
  score: number;
}

export function getMemoryOverview(): Promise<MemoryOverview> {
  return request<MemoryOverview>('/memory/overview');
}

export function getRecentBlocks(limit?: number): Promise<ScopedBlock[]> {
  const qs = limit ? `?limit=${limit}` : '';
  return request<ScopedBlock[]>(`/memory/recent${qs}`);
}

export function searchMemoryBlocks(query: string, limit?: number): Promise<MemorySearchResult[]> {
  const params = new URLSearchParams({ query });
  if (limit !== undefined) params.set('limit', String(limit));
  return request<MemorySearchResult[]>(`/memory/search?${params.toString()}`);
}

export function getExplorerGraph(): Promise<ExplorerGraphData> {
  return request<ExplorerGraphData>('/memory/explorer-graph');
}
