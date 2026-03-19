import { useAgentStatus } from '@/hooks/useAgentStatus';
import { cn } from '@/lib/utils';

interface AgentStatusBadgeProps {
  agentId: string;
  staticStatus?: string;
  className?: string;
}

function statusVariant(status: string | null): string {
  switch (status) {
    case 'running':
      return 'bg-sera-success/15 text-sera-success';
    case 'error':
      return 'bg-sera-error/15 text-sera-error';
    case 'unresponsive':
      return 'bg-sera-warning/15 text-sera-warning';
    default:
      return 'bg-sera-surface-hover text-sera-text-muted';
  }
}

export function AgentStatusBadge({ agentId, staticStatus, className }: AgentStatusBadgeProps) {
  const liveStatus = useAgentStatus(agentId);
  const status = liveStatus ?? staticStatus ?? 'stopped';

  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md text-[11px] font-medium tracking-wide uppercase',
        'transition-colors duration-300',
        statusVariant(status),
        className,
      )}
    >
      <span
        className={cn(
          'h-1.5 w-1.5 rounded-full transition-colors duration-300',
          status === 'running' && 'bg-sera-success',
          status === 'error' && 'bg-sera-error',
          status === 'unresponsive' && 'bg-sera-warning',
          status !== 'running' && status !== 'error' && status !== 'unresponsive' && 'bg-sera-text-muted',
        )}
      />
      {status}
    </span>
  );
}
