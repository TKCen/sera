import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { request } from '@/lib/api/client';
import {
  ChevronDown,
  ChevronRight,
  Clock,
  Terminal,
  CheckCircle2,
  XCircle,
  Hash
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { TabLoading } from '@/components/AgentDetailTabLoading';

interface CommandLog {
  id: string;
  sessionId: string;
  toolName: string;
  arguments: Record<string, unknown>;
  result: string;
  durationMs: number;
  status: 'success' | 'error';
  createdAt: string;
}

export function CommandLogTimeline({ agentId, sessionId }: { agentId: string, sessionId: string }) {
  const { data, isLoading } = useQuery({
    queryKey: ['agent-command-logs', agentId, sessionId],
    queryFn: () => request<CommandLog[]>(`/agents/${encodeURIComponent(agentId)}/sessions/${encodeURIComponent(sessionId)}/commands`),
    enabled: !!agentId && !!sessionId,
  });

  if (isLoading) return <TabLoading />;

  if (!data || data.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center p-12 text-sera-text-muted">
        <Terminal size={32} className="mb-3 opacity-20" />
        <p className="text-sm font-medium">No command logs found for this session.</p>
        <p className="text-xs mt-1">Enable "spec.logging.commands: true" in the agent manifest to see tool logs.</p>
      </div>
    );
  }

  return (
    <div className="p-6 max-w-4xl space-y-4">
      <div className="flex items-center justify-between mb-4">
        <h3 className="text-sm font-semibold text-sera-text">Command Execution Log</h3>
        <span className="text-[10px] font-mono text-sera-text-dim uppercase tracking-wider">
          {data.length} tool calls
        </span>
      </div>

      <div className="space-y-3">
        {data.map((log) => (
          <CommandLogItem key={log.id} log={log} />
        ))}
      </div>
    </div>
  );
}

function CommandLogItem({ log }: { log: CommandLog }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className={cn(
      "sera-card-static overflow-hidden transition-all duration-200",
      expanded ? "ring-1 ring-sera-border-bright bg-sera-surface-bright/30" : "hover:bg-sera-surface/50"
    )}>
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center gap-3 px-4 py-3 text-left group"
      >
        <div className="flex-shrink-0">
          {log.status === 'success' ? (
            <CheckCircle2 size={16} className="text-sera-success" />
          ) : (
            <XCircle size={16} className="text-sera-error" />
          )}
        </div>

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-sm font-mono font-semibold text-sera-text truncate">
              {log.toolName}
            </span>
            <span className={cn(
              "text-[10px] px-1.5 py-0.5 rounded uppercase font-bold",
              log.status === 'success' ? "bg-sera-success/10 text-sera-success/80" : "bg-sera-error/10 text-sera-error/80"
            )}>
              {log.status}
            </span>
          </div>
          <div className="flex items-center gap-3 mt-1">
            <span className="flex items-center gap-1 text-[10px] text-sera-text-muted">
              <Clock size={10} /> {log.durationMs}ms
            </span>
            <span className="flex items-center gap-1 text-[10px] text-sera-text-muted font-mono">
              <Hash size={10} /> {new Date(log.createdAt).toLocaleTimeString()}
            </span>
          </div>
        </div>

        <div className="flex-shrink-0 text-sera-text-muted group-hover:text-sera-text transition-colors">
          {expanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
        </div>
      </button>

      {expanded && (
        <div className="px-4 pb-4 pt-1 space-y-4 border-t border-sera-border/40 bg-black/20">
          <section>
            <h4 className="text-[10px] font-bold text-sera-text-muted uppercase mb-2 flex items-center gap-1.5">
              Arguments
            </h4>
            <pre className="p-3 rounded bg-black/40 border border-sera-border/30 text-xs font-mono text-sera-accent overflow-x-auto">
              {JSON.stringify(log.arguments, null, 2)}
            </pre>
          </section>

          <section>
            <h4 className="text-[10px] font-bold text-sera-text-muted uppercase mb-2 flex items-center gap-1.5">
              Result
            </h4>
            <div className="p-3 rounded bg-black/40 border border-sera-border/30 text-xs font-mono text-sera-text-dim whitespace-pre-wrap break-words max-h-96 overflow-y-auto">
              {log.result || <em className="italic opacity-50">No output</em>}
            </div>
          </section>
        </div>
      )}
    </div>
  );
}
