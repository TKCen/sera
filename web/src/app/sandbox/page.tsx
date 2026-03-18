'use client';

import {
  Box,
  RefreshCw,
  Terminal,
  Trash2,
  AlertCircle,
  Activity,
  Shield,
  Clock,
  X
} from 'lucide-react';
import { useState, useEffect } from 'react';

interface SandboxInfo {
  containerId: string;
  agentName: string;
  type: 'agent' | 'subagent' | 'tool';
  image: string;
  status: 'running' | 'stopped' | 'removing';
  createdAt: string;
  tier: number;
  parentAgent?: string;
  subagentRole?: string;
}

export default function SandboxPage() {
  const [containers, setContainers] = useState<SandboxInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [logsContainerId, setLogsContainerId] = useState<string | null>(null);
  const [logs, setLogs] = useState<string>('');
  const [logsLoading, setLogsLoading] = useState(false);

  const handleRemove = async (containerId: string, agentName: string) => {
    if (!confirm(`Are you sure you want to stop and remove container ${containerId.substring(0, 12)}?`)) {
      return;
    }

    try {
      const res = await fetch(`/api/core/sandbox/${containerId}?agentName=${agentName}`, {
        method: 'DELETE',
      });

      if (!res.ok) {
        const data = await res.json();
        throw new Error(data.error || 'Failed to remove container');
      }

      fetchContainers();
    } catch (err: unknown) {
      alert(err instanceof Error ? err.message : String(err));
    }
  };

  const fetchLogs = async (containerId: string) => {
    try {
      setLogsLoading(true);
      setLogsContainerId(containerId);
      const res = await fetch(`/api/core/sandbox/${containerId}/logs?tail=200`);
      if (!res.ok) throw new Error('Failed to fetch logs');
      const data = await res.json();
      setLogs(data.logs);
    } catch (err: unknown) {
      setLogs(`Error: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setLogsLoading(false);
    }
  };

  const fetchContainers = async () => {
    try {
      setLoading(true);
      const res = await fetch('/api/core/sandbox');
      if (!res.ok) throw new Error('Failed to fetch sandbox containers');
      const data = await res.json();
      setContainers(data);
      setError(null);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchContainers();
  }, []);

  const formatUptime = (createdAt: string) => {
    const created = new Date(createdAt).getTime();
    const now = new Date().getTime();
    const diff = Math.floor((now - created) / 1000);

    if (diff < 60) return `${diff}s`;
    if (diff < 3600) return `${Math.floor(diff / 60)}m ${diff % 60}s`;
    return `${Math.floor(diff / 3600)}h ${Math.floor((diff % 3600) / 60)}m`;
  };

  return (
    <div className="p-8 max-w-6xl mx-auto">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Sandbox Manager</h1>
          <p className="text-sm text-sera-text-muted mt-1">Manage isolated agent environments and containers</p>
        </div>
        <button
          onClick={fetchContainers}
          disabled={loading}
          className="sera-btn-ghost flex items-center gap-2"
        >
          <RefreshCw size={16} className={loading ? 'animate-spin' : ''} />
          Refresh
        </button>
      </div>

      {error && (
        <div className="p-4 mb-6 border border-sera-error/30 bg-sera-error/5 rounded-lg flex items-center gap-3 text-sera-error text-sm">
          <AlertCircle size={18} />
          {error}
        </div>
      )}

      {loading && containers.length === 0 ? (
        <div className="flex items-center justify-center py-20">
          <RefreshCw size={24} className="animate-spin text-sera-accent" />
        </div>
      ) : containers.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24 sera-card-static border-dashed">
          <div className="w-16 h-16 rounded-2xl bg-sera-surface border border-sera-border flex items-center justify-center mb-5">
            <Box size={28} className="text-sera-text-dim" />
          </div>
          <h2 className="text-lg font-semibold text-sera-text mb-2">No active containers</h2>
          <p className="text-sm text-sera-text-muted text-center max-w-md">
            Active agent containers and tool sandboxes will appear here once they are spawned.
          </p>
        </div>
      ) : (
        <div className="sera-card-static overflow-hidden">
          <table className="w-full text-left border-collapse">
            <thead>
              <tr className="bg-sera-surface-hover/50 border-b border-sera-border">
                <th className="px-5 py-3 text-[10px] font-semibold uppercase tracking-wider text-sera-text-dim">Agent / Role</th>
                <th className="px-5 py-3 text-[10px] font-semibold uppercase tracking-wider text-sera-text-dim">Type</th>
                <th className="px-5 py-3 text-[10px] font-semibold uppercase tracking-wider text-sera-text-dim">Security</th>
                <th className="px-5 py-3 text-[10px] font-semibold uppercase tracking-wider text-sera-text-dim">Image</th>
                <th className="px-5 py-3 text-[10px] font-semibold uppercase tracking-wider text-sera-text-dim">Status</th>
                <th className="px-5 py-3 text-[10px] font-semibold uppercase tracking-wider text-sera-text-dim">Uptime</th>
                <th className="px-5 py-3 text-[10px] font-semibold uppercase tracking-wider text-sera-text-dim text-right">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-sera-border">
              {containers.map((c) => (
                <tr key={c.containerId} className="hover:bg-sera-surface-hover/30 transition-colors">
                  <td className="px-5 py-4">
                    <div className="font-medium text-sera-text text-sm">{c.agentName}</div>
                    {c.subagentRole && (
                      <div className="text-[11px] text-sera-text-muted font-mono mt-0.5">{c.subagentRole}</div>
                    )}
                  </td>
                  <td className="px-5 py-4">
                    <span className={`sera-badge-${c.type === 'tool' ? 'warning' : 'accent'} text-[9px]`}>
                      {c.type}
                    </span>
                  </td>
                  <td className="px-5 py-4">
                    <div className="flex items-center gap-1.5">
                      <Shield size={12} className={c.tier === 3 ? 'text-sera-error' : c.tier === 2 ? 'text-sera-warning' : 'text-sera-success'} />
                      <span className="text-xs font-medium text-sera-text">Tier {c.tier}</span>
                    </div>
                  </td>
                  <td className="px-5 py-4">
                    <div className="text-xs text-sera-text-muted font-mono truncate max-w-[150px]" title={c.image}>
                      {c.image.split('/').pop()}
                    </div>
                  </td>
                  <td className="px-5 py-4">
                    <span className={`flex items-center gap-1.5 text-[10px] font-medium uppercase tracking-wider ${
                      c.status === 'running' ? 'text-sera-success' : 'text-sera-text-dim'
                    }`}>
                      <Activity size={10} className={c.status === 'running' ? 'animate-pulse' : ''} />
                      {c.status}
                    </span>
                  </td>
                  <td className="px-5 py-4">
                    <div className="flex items-center gap-1.5 text-xs text-sera-text-muted">
                      <Clock size={12} />
                      {formatUptime(c.createdAt)}
                    </div>
                  </td>
                  <td className="px-5 py-4 text-right">
                    <div className="flex items-center justify-end gap-2">
                      <button
                        onClick={() => fetchLogs(c.containerId)}
                        className="p-1.5 text-sera-text-muted hover:text-sera-accent hover:bg-sera-accent/10 rounded-md transition-colors"
                        title="View Logs"
                      >
                        <Terminal size={16} />
                      </button>
                      <button
                        onClick={() => handleRemove(c.containerId, c.agentName)}
                        className="p-1.5 text-sera-text-muted hover:text-sera-error hover:bg-sera-error/10 rounded-md transition-colors"
                        title="Stop & Remove"
                      >
                        <Trash2 size={16} />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {/* Logs Modal */}
      {logsContainerId && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-sera-bg/80 backdrop-blur-sm">
          <div className="sera-card-static w-full max-w-4xl h-[80vh] flex flex-col shadow-2xl animate-in zoom-in-95 duration-200">
            {/* Modal Header */}
            <div className="px-6 py-4 border-b border-sera-border flex items-center justify-between bg-sera-surface-hover/50 rounded-t-xl">
              <div className="flex items-center gap-3">
                <div className="w-8 h-8 rounded-lg bg-sera-accent/10 flex items-center justify-center">
                  <Terminal size={18} className="text-sera-accent" />
                </div>
                <div>
                  <h3 className="text-sm font-semibold text-sera-text">Container Logs</h3>
                  <p className="text-[10px] text-sera-text-muted font-mono">{logsContainerId}</p>
                </div>
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => fetchLogs(logsContainerId)}
                  disabled={logsLoading}
                  className="p-2 text-sera-text-muted hover:text-sera-accent hover:bg-sera-accent/10 rounded-md transition-colors"
                  title="Refresh Logs"
                >
                  <RefreshCw size={16} className={logsLoading ? 'animate-spin' : ''} />
                </button>
                <button
                  onClick={() => {
                    setLogsContainerId(null);
                    setLogs('');
                  }}
                  className="p-2 text-sera-text-muted hover:text-sera-error hover:bg-sera-error/10 rounded-md transition-colors"
                >
                  <X size={18} />
                </button>
              </div>
            </div>

            {/* Logs Content */}
            <div className="flex-1 overflow-auto p-6 bg-black font-mono text-[13px] leading-relaxed text-sera-text/90">
              {logsLoading && logs === '' ? (
                <div className="flex items-center justify-center h-full">
                  <RefreshCw size={24} className="animate-spin text-sera-accent" />
                </div>
              ) : (
                <pre className="whitespace-pre-wrap">
                  {logs || 'No logs available for this container.'}
                </pre>
              )}
            </div>

            {/* Footer */}
            <div className="px-6 py-3 border-t border-sera-border text-[10px] text-sera-text-dim flex justify-between">
              <span>Showing last 200 lines</span>
              <span>Press ESC to close</span>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
