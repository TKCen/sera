import { Check, AlertCircle, RotateCcw } from 'lucide-react';
import { useAgentHealthCheck } from '@/hooks/useAgents';
import { Button } from '@/components/ui/button';
import { TabLoading } from '@/components/AgentDetailTabLoading';
import { cn } from '@/lib/utils';

export function AgentDetailHealthCheckTab({ id }: { id: string }) {
  const { data, isLoading, refetch, isFetching } = useAgentHealthCheck(id);

  if (isLoading) return <TabLoading />;

  const overallColor =
    data?.overallStatus === 'healthy'
      ? 'text-sera-success'
      : data?.overallStatus === 'degraded'
        ? 'text-yellow-400'
        : 'text-sera-error';

  return (
    <div className="p-6 max-w-2xl space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h3 className="text-sm font-semibold text-sera-text">Health Check</h3>
          <p className="text-xs text-sera-text-muted mt-0.5">
            Diagnostic checks for this agent instance
          </p>
        </div>
        <Button size="sm" variant="outline" onClick={() => void refetch()} disabled={isFetching}>
          <RotateCcw size={12} className={isFetching ? 'animate-spin' : ''} /> Re-check
        </Button>
      </div>

      {data && (
        <>
          <div className={cn('text-sm font-semibold', overallColor)}>
            Status: {data.overallStatus}
          </div>

          <div className="sera-card-static divide-y divide-sera-border/50">
            {Object.entries(data.checks).map(([name, check]) => (
              <div key={name} className="flex items-center gap-3 px-4 py-3">
                {check.ok ? (
                  <Check size={14} className="text-sera-success flex-shrink-0" />
                ) : (
                  <AlertCircle size={14} className="text-sera-error flex-shrink-0" />
                )}
                <span className="text-sm text-sera-text font-medium flex-1">{name}</span>
                {check.detail && (
                  <span className="text-xs text-sera-text-muted">{check.detail}</span>
                )}
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
