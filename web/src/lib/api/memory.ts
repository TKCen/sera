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
