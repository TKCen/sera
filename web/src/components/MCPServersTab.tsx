import { useState } from 'react';
import { toast } from 'sonner';
import {
  Server,
  RefreshCw,
  Trash2,
  HeartPulse,
  Wrench,
  Plus,
  AlertCircle,
  CheckCircle2,
  XCircle,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';
import { useMCPServers, useUnregisterMCPServer, useReloadMCPServer } from '@/hooks/useMCPServers';
import { MCPServerDialog } from '@/components/MCPServerDialog';
import type { MCPServerInfo } from '@/lib/api/mcp';
import { getMCPServerHealth } from '@/lib/api/mcp';
import { cn } from '@/lib/utils';

function StatusBadge({ status }: { status: MCPServerInfo['status'] }) {
  const variants: Record<MCPServerInfo['status'], { icon: React.ReactNode; className: string }> = {
    connected: {
      icon: <CheckCircle2 size={10} />,
      className: 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30',
    },
    disconnected: {
      icon: <AlertCircle size={10} />,
      className: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30',
    },
    error: {
      icon: <XCircle size={10} />,
      className: 'bg-red-500/15 text-red-400 border-red-500/30',
    },
  };
  const v = variants[status];
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-medium border',
        v.className
      )}
    >
      {v.icon}
      {status}
    </span>
  );
}

function ServerCard({
  server,
  onUnregister,
  onReload,
  onHealthCheck,
}: {
  server: MCPServerInfo;
  onUnregister: (name: string) => void;
  onReload: (name: string) => void;
  onHealthCheck: (name: string) => void;
}) {
  return (
    <div className="sera-card p-5 flex flex-col gap-3 group">
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-3">
          <div className="w-9 h-9 rounded-lg bg-sera-accent/10 flex items-center justify-center flex-shrink-0">
            <Server size={16} className="text-sera-accent" />
          </div>
          <div>
            <h3 className="font-mono text-sm font-medium text-sera-text">{server.name}</h3>
            <div className="flex items-center gap-2 mt-0.5">
              <StatusBadge status={server.status} />
            </div>
          </div>
        </div>
      </div>

      <div className="flex items-center gap-4 text-xs text-sera-text-muted">
        <span className="flex items-center gap-1">
          <Wrench size={11} /> {server.toolCount} tool{server.toolCount !== 1 ? 's' : ''}
        </span>
      </div>

      <div className="flex items-center gap-2 mt-auto pt-2 border-t border-sera-border/30">
        <Button
          size="sm"
          variant="ghost"
          className="h-7 text-[11px] gap-1"
          onClick={() => onHealthCheck(server.name)}
        >
          <HeartPulse size={12} /> Health
        </Button>
        <Button
          size="sm"
          variant="ghost"
          className="h-7 text-[11px] gap-1"
          onClick={() => onReload(server.name)}
        >
          <RefreshCw size={12} /> Reload
        </Button>
        <div className="flex-1" />
        <Button
          size="sm"
          variant="ghost"
          className="h-7 text-[11px] gap-1 text-red-400 hover:text-red-300 opacity-0 group-hover:opacity-100 transition-opacity"
          onClick={() => onUnregister(server.name)}
        >
          <Trash2 size={12} /> Remove
        </Button>
      </div>
    </div>
  );
}

function StatBox({ label, value }: { label: string; value: number | string }) {
  return (
    <div className="sera-card-static px-4 py-3 text-center">
      <div className="text-lg font-bold text-sera-text">{value}</div>
      <div className="text-[10px] uppercase tracking-wider text-sera-text-dim">{label}</div>
    </div>
  );
}

export function MCPServersTab() {
  const { data: servers, isLoading } = useMCPServers();
  const unregister = useUnregisterMCPServer();
  const reload = useReloadMCPServer();
  const [showRegister, setShowRegister] = useState(false);

  function handleUnregister(name: string) {
    if (!confirm(`Unregister MCP server "${name}"? Its tools will be removed.`)) return;
    unregister.mutate(name, {
      onSuccess: () => toast.success(`Server "${name}" unregistered`),
      onError: (err) => toast.error(err instanceof Error ? err.message : 'Unregister failed'),
    });
  }

  function handleReload(name: string) {
    reload.mutate(name, {
      onSuccess: (data) =>
        toast.success(`Server "${name}" reloaded — ${data.toolCount} tools available`),
      onError: (err) => toast.error(err instanceof Error ? err.message : 'Reload failed'),
    });
  }

  async function handleHealthCheck(name: string) {
    try {
      const health = await getMCPServerHealth(name);
      if (health.healthy) {
        toast.success(`${name}: healthy (${health.toolCount} tools)`);
      } else {
        toast.error(`${name}: unhealthy — ${health.error}`);
      }
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Health check failed');
    }
  }

  const connectedCount = (servers ?? []).filter((s) => s.status === 'connected').length;
  const totalTools = (servers ?? []).reduce((sum, s) => sum + s.toolCount, 0);

  return (
    <div className="space-y-6 animate-in fade-in slide-in-from-bottom-2 duration-300">
      <div className="flex items-center justify-between">
        <div>
          <p className="text-sm text-sera-text-muted">
            Manage Model Context Protocol servers that provide tools to agents.
          </p>
        </div>
        <Button size="sm" onClick={() => setShowRegister(true)}>
          <Plus size={12} /> Register Server
        </Button>
      </div>

      {!isLoading && servers && servers.length > 0 && (
        <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
          <StatBox label="Total Servers" value={servers.length} />
          <StatBox label="Connected" value={connectedCount} />
          <StatBox label="Total Tools" value={totalTools} />
          <StatBox label="Errors" value={servers.filter((s) => s.status === 'error').length} />
        </div>
      )}

      {isLoading ? (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-40 rounded-xl" />
          ))}
        </div>
      ) : !servers || servers.length === 0 ? (
        <EmptyState
          icon={<Server size={24} />}
          title="No MCP servers registered"
          description="Register an MCP server to make its tools available to agents. Servers are defined using YAML manifests in the mcp-servers/ directory."
        />
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {servers.map((server) => (
            <ServerCard
              key={server.name}
              server={server}
              onUnregister={handleUnregister}
              onReload={handleReload}
              onHealthCheck={handleHealthCheck}
            />
          ))}
        </div>
      )}

      <MCPServerDialog open={showRegister} onOpenChange={setShowRegister} />
    </div>
  );
}
