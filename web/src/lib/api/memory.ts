import { request } from './client';
import type { MemoryEntry } from './types';

// ── Legacy Letta-style memory entry creation (kept for AgentDetailMemoryTab) ──

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

export function updateAgentBlock(
  agentId: string,
  blockId: string,
  updates: { title?: string; content?: string; tags?: string[]; importance?: number }
): Promise<ScopedBlock> {
  return request<ScopedBlock>(
    `/memory/${encodeURIComponent(agentId)}/blocks/${encodeURIComponent(blockId)}`,
    { method: 'PUT', body: JSON.stringify(updates) }
  );
}

export function deleteAgentBlock(agentId: string, blockId: string): Promise<void> {
  return request<void>(
    `/memory/${encodeURIComponent(agentId)}/blocks/${encodeURIComponent(blockId)}`,
    { method: 'DELETE' }
  );
}

export function triggerCompaction(agentId: string): Promise<{ compacted: number }> {
  return request(`/memory/${encodeURIComponent(agentId)}/compact`, { method: 'POST' });
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

// ── Core Memory Blocks (Story 8.1) ───────────────────────────────────────

export interface CoreMemoryBlock {
  id: string;
  agentInstanceId: string;
  name: string;
  content: string;
  characterLimit: number;
  isReadOnly: boolean;
  createdAt: string;
  updatedAt: string;
}

export function getCoreMemoryBlocks(agentId: string): Promise<CoreMemoryBlock[]> {
  return request<CoreMemoryBlock[]>(`/memory/${encodeURIComponent(agentId)}/core`);
}

export function updateCoreMemoryBlock(
  agentId: string,
  name: string,
  updates: { content?: string; characterLimit?: number; isReadOnly?: boolean }
): Promise<CoreMemoryBlock> {
  return request<CoreMemoryBlock>(
    `/memory/${encodeURIComponent(agentId)}/core/${encodeURIComponent(name)}`,
    {
      method: 'PUT',
      body: JSON.stringify(updates),
    }
  );
}
