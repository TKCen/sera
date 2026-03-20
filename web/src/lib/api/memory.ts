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
