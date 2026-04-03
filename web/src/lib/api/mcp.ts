import { request } from './client';

export interface MCPServerInfo {
  name: string;
  status: 'connected' | 'disconnected' | 'error';
  toolCount: number;
}

export interface MCPServerDetail extends MCPServerInfo {
  tools: Array<{ name: string; description?: string }>;
}

export interface MCPServerHealth {
  name: string;
  healthy: boolean;
  toolCount?: number;
  error?: string;
  checkedAt: string;
}

export function listMCPServers(): Promise<MCPServerInfo[]> {
  return request<MCPServerInfo[]>('/mcp-servers');
}

export function getMCPServer(name: string): Promise<MCPServerDetail> {
  return request<MCPServerDetail>(`/mcp-servers/${encodeURIComponent(name)}`);
}

export function getMCPServerHealth(name: string): Promise<MCPServerHealth> {
  return request<MCPServerHealth>(`/mcp-servers/${encodeURIComponent(name)}/health`);
}

export function reloadMCPServer(name: string): Promise<{ message: string; toolCount: number }> {
  return request<{ message: string; toolCount: number }>(
    `/mcp-servers/${encodeURIComponent(name)}/reload`,
    { method: 'POST' }
  );
}

export function registerMCPServer(manifest: object): Promise<{ message: string }> {
  return request<{ message: string }>('/mcp-servers', {
    method: 'POST',
    body: JSON.stringify(manifest),
  });
}

export function unregisterMCPServer(name: string): Promise<{ message: string }> {
  return request<{ message: string }>(`/mcp-servers/${encodeURIComponent(name)}`, {
    method: 'DELETE',
  });
}
