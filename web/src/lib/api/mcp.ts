import { request } from './client';

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
