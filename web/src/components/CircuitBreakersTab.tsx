import { RefreshCw } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';

export interface CircuitBreakerData {
  provider: string;
  state: string;
  failures: number;
  lastFailureAt?: string;
  nextRetryAt?: string;
}

export interface CircuitBreakersTabProps {
  breakers: CircuitBreakerData[];
  onReset: (provider: string) => void;
  resetting: boolean;
}

function cbStateBadge(state: string) {
  if (state === 'open') return <Badge variant="error">Open</Badge>;
  if (state === 'half-open') return <Badge variant="warning">Half-Open</Badge>;
  return <Badge variant="success">Closed</Badge>;
}

export function CircuitBreakersTab({
  breakers,
  onReset,
  resetting,
}: CircuitBreakersTabProps) {
  return (
    <div className="space-y-4">
      <p className="text-sm text-sera-text-muted">
        Circuit breakers protect against repeated failures. When open, requests to the provider are
        paused.
      </p>
      {breakers.length === 0 ? (
        <div className="sera-card-static p-8 text-center text-sera-text-dim text-sm">
          No circuit breaker data — all providers healthy.
        </div>
      ) : (
        <div className="sera-card-static overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-sera-border text-[11px] uppercase tracking-wider text-sera-text-dim">
                <th className="text-left py-3 px-4">Provider</th>
                <th className="text-left py-3 px-4">State</th>
                <th className="text-left py-3 px-4">Failures</th>
                <th className="text-left py-3 px-4">Last Failure</th>
                <th className="text-left py-3 px-4">Next Retry</th>
                <th className="py-3 px-4" />
              </tr>
            </thead>
            <tbody>
              {breakers.map((cb) => (
                <tr
                  key={cb.provider}
                  className="border-b border-sera-border/50 hover:bg-sera-surface-hover transition-colors"
                >
                  <td className="py-3 px-4 font-mono text-xs text-sera-text">{cb.provider}</td>
                  <td className="py-3 px-4">{cbStateBadge(cb.state)}</td>
                  <td className="py-3 px-4 text-sera-text-muted">{cb.failures}</td>
                  <td className="py-3 px-4 text-xs text-sera-text-muted">
                    {cb.lastFailureAt ? new Date(cb.lastFailureAt).toLocaleString() : '—'}
                  </td>
                  <td className="py-3 px-4 text-xs text-sera-text-muted">
                    {cb.nextRetryAt ? new Date(cb.nextRetryAt).toLocaleString() : '—'}
                  </td>
                  <td className="py-3 px-4 text-right">
                    {cb.state !== 'closed' && (
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={resetting}
                        onClick={() => onReset(cb.provider)}
                      >
                        <RefreshCw size={12} /> Reset
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
