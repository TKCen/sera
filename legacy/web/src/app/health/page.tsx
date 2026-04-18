import { RefreshCw, CheckCircle2, AlertTriangle, XCircle, Bot } from 'lucide-react';
import { useHealthDetail } from '@/hooks/useHealth';
import { Spinner } from '@/components/ui/spinner';
import type { ComponentHealth } from '@/lib/api/types';

function statusIcon(status: ComponentHealth['status']) {
  if (status === 'healthy') return <CheckCircle2 size={16} className="text-sera-success" />;
  if (status === 'degraded') return <AlertTriangle size={16} className="text-sera-warning" />;
  return <XCircle size={16} className="text-sera-error" />;
}

function statusLabel(status: ComponentHealth['status']) {
  if (status === 'healthy') return 'Healthy';
  if (status === 'degraded') return 'Degraded';
  return 'Unreachable';
}

function statusColor(status: string) {
  if (status === 'healthy') return 'text-sera-success';
  if (status === 'degraded') return 'text-sera-warning';
  return 'text-sera-error';
}

export default function HealthPage() {
  const { data, isLoading, refetch, isFetching } = useHealthDetail();

  return (
    <div className="p-8 max-w-4xl mx-auto space-y-8">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">System Health</h1>
          <p className="text-sm text-sera-text-muted mt-1">Component status refreshes every 30s</p>
        </div>
        <button
          onClick={() => {
            void refetch();
          }}
          className="sera-btn-ghost p-2"
          title="Refresh"
        >
          <RefreshCw size={14} className={isFetching ? 'animate-spin' : ''} />
        </button>
      </div>

      {isLoading ? (
        <div className="flex items-center justify-center py-20">
          <Spinner />
        </div>
      ) : (
        <>
          {/* Overall status */}
          <div
            className={`flex items-center gap-3 px-5 py-4 rounded-xl border ${
              data?.status === 'healthy'
                ? 'bg-sera-success/10 border-sera-success/30'
                : data?.status === 'degraded'
                  ? 'bg-sera-warning/10 border-sera-warning/30'
                  : 'bg-sera-error/10 border-sera-error/30'
            }`}
          >
            {data?.status === 'healthy' ? (
              <CheckCircle2 size={20} className="text-sera-success" />
            ) : data?.status === 'degraded' ? (
              <AlertTriangle size={20} className="text-sera-warning" />
            ) : (
              <XCircle size={20} className="text-sera-error" />
            )}
            <div>
              <p className={`font-semibold ${statusColor(data?.status ?? 'unreachable')}`}>
                {data?.status === 'healthy'
                  ? 'All systems operational'
                  : data?.status === 'degraded'
                    ? 'Some components degraded'
                    : 'System unhealthy'}
              </p>
              {data?.timestamp && (
                <p className="text-xs text-sera-text-dim mt-0.5">
                  Last checked: {new Date(data.timestamp).toLocaleString()}
                </p>
              )}
            </div>
          </div>

          {/* Component grid */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {(data?.components ?? []).map((comp) => (
              <div key={comp.name} className="sera-card-static p-4 flex items-start gap-3">
                <div className="mt-0.5">{statusIcon(comp.status)}</div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center justify-between">
                    <span className="text-sm font-medium text-sera-text">{comp.name}</span>
                    <span className={`text-xs font-medium ${statusColor(comp.status)}`}>
                      {statusLabel(comp.status)}
                    </span>
                  </div>
                  {comp.message && (
                    <p className="text-xs text-sera-text-muted mt-1">{comp.message}</p>
                  )}
                  {comp.latencyMs !== undefined && (
                    <p className="text-[11px] text-sera-text-dim mt-1">
                      Latency: {comp.latencyMs}ms
                    </p>
                  )}
                </div>
              </div>
            ))}
          </div>

          {/* Agent stats */}
          {data?.agentStats && (
            <div className="sera-card-static p-5">
              <div className="flex items-center gap-2 mb-4">
                <Bot size={14} className="text-sera-accent" />
                <h2 className="text-sm font-semibold text-sera-text">Agent Stats</h2>
              </div>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
                {(
                  [
                    ['Total', data.agentStats.total, 'text-sera-text'],
                    ['Running', data.agentStats.running, 'text-sera-success'],
                    ['Stopped', data.agentStats.stopped, 'text-sera-text-muted'],
                    ['Errored', data.agentStats.errored, 'text-sera-error'],
                  ] as [string, number, string][]
                ).map(([label, value, color]) => (
                  <div key={label} className="text-center">
                    <p className={`text-2xl font-bold ${color}`}>{value}</p>
                    <p className="text-xs text-sera-text-dim mt-1">{label}</p>
                  </div>
                ))}
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}
