import { useAgentDelegations, useAgentTasks } from '@/hooks/useAgents';
import { TabLoading } from './AgentDetailTabLoading';
import { Badge } from './ui/badge';
import { ShieldCheck, User, Clock, Activity, ArrowRight, ArrowLeft } from 'lucide-react';
import { formatDistanceToNow } from '@/lib/utils';


export function DelegationsTab({ id }: { id: string }) {
  const { data: delegations, isLoading } = useAgentDelegations(id);
  const { data: tasks } = useAgentTasks(id, 'all');

  // Separate delegated tasks: tasks received from other agents vs tasks this agent sent
  const receivedTasks = (tasks ?? []).filter((t) => {
    const ctx = t.context as Record<string, unknown> | null;
    return ctx?.delegation;
  });

  if (isLoading) return <TabLoading />;

  if (!delegations || delegations.length === 0) {
    return (
      <div className="p-8 text-center">
        <div className="mx-auto w-12 h-12 rounded-full bg-sera-surface-hover flex items-center justify-center mb-3">
          <ShieldCheck size={24} className="text-sera-text-dim" />
        </div>
        <h3 className="text-sm font-medium text-sera-text">No inbound delegations</h3>
        <p className="text-xs text-sera-text-dim mt-1 max-w-xs mx-auto">
          This agent hasn't received any delegated authority from operators or other agents yet.
        </p>
      </div>
    );
  }

  return (
    <div className="p-6 space-y-6 max-w-5xl">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
          <ShieldCheck size={14} className="text-sera-accent" />
          Inbound Delegations
        </h3>
        <span className="text-xs text-sera-text-dim">{delegations.length} active</span>
      </div>

      <div className="grid gap-4">
        {delegations.map((del) => (
          <div
            key={del.id}
            className="sera-card-static p-4 hover:border-sera-accent/30 transition-colors"
          >
            <div className="flex items-start justify-between gap-4">
              <div className="space-y-3 flex-1">
                {/* Header: Source & Status */}
                <div className="flex items-center gap-3">
                  <div className="h-8 w-8 rounded-full bg-sera-accent-soft flex items-center justify-center shrink-0">
                    <User size={14} className="text-sera-accent" />
                  </div>
                  <div>
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium text-sera-text">
                        {del.principal_name}
                      </span>
                      <Badge
                        variant={del.status === 'active' ? 'accent' : 'default'}
                        className="text-[10px] h-4"
                      >
                        {del.status}
                      </Badge>
                    </div>
                    <div className="text-[10px] text-sera-text-dim font-mono">
                      {del.principal_id}
                    </div>
                  </div>
                </div>

                {/* Scope */}
                <div className="bg-sera-surface-soft rounded-lg p-3 space-y-2 border border-sera-border/50">
                  <div className="flex items-center gap-2 text-[10px] uppercase tracking-wider font-semibold text-sera-text-dim">
                    Scope: <span className="text-sera-accent">{del.scope.service}</span>
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    {del.scope.permissions.map((p: string) => (
                      <code
                        key={p}
                        className="text-[10px] px-1.5 py-0.5 rounded bg-sera-surface border border-sera-border text-sera-text"
                      >
                        {p}
                      </code>
                    ))}
                  </div>
                  {del.scope.resourceConstraints &&
                    Object.keys(del.scope.resourceConstraints).length > 0 && (
                      <div className="mt-2 pt-2 border-t border-sera-border/30">
                        <div className="text-[9px] uppercase text-sera-text-dim mb-1">
                          Constraints
                        </div>
                        <div className="space-y-1">
                          {Object.entries(del.scope.resourceConstraints).map(
                            ([k, v]: [string, string[]]) => (
                              <div key={k} className="text-[10px] flex gap-2">
                                <span className="text-sera-text-muted">{k}:</span>
                                <span className="text-sera-text font-mono">{v.join(', ')}</span>
                              </div>
                            )
                          )}
                        </div>
                      </div>
                    )}
                </div>
              </div>

              {/* Metadata */}
              <div className="w-48 space-y-3 shrink-0 text-right border-l border-sera-border/50 pl-4">
                <div className="space-y-1">
                  <div className="flex items-center justify-end gap-1.5 text-[10px] text-sera-text-dim">
                    <Clock size={10} /> Issued
                  </div>
                  <div className="text-[10px] text-sera-text">
                    {new Date(del.issued_at).toLocaleString()}
                  </div>
                </div>

                {del.expires_at && (
                  <div className="space-y-1">
                    <div className="flex items-center justify-end gap-1.5 text-[10px] text-sera-text-dim">
                      <Activity size={10} /> Expires
                    </div>
                    <div className="text-[10px] text-sera-text">
                      {new Date(del.expires_at).toLocaleString()}
                    </div>
                  </div>
                )}

                <div className="space-y-1">
                  <div className="text-[10px] text-sera-text-dim uppercase tracking-tighter">
                    Grant Type
                  </div>
                  <Badge variant="default" className="text-[9px] h-4 uppercase">
                    {del.grant_type}
                  </Badge>
                </div>

                <div className="pt-2 border-t border-sera-border/30">
                  <div className="text-[9px] text-sera-text-dim">Used {del.use_count} times</div>
                  {del.last_used_at && (
                    <div className="text-[9px] text-sera-text-muted">
                      Last: {new Date(del.last_used_at).toLocaleDateString()}
                    </div>
                  )}
                </div>
              </div>
            </div>
          </div>
        ))}
      </div>

      {/* Task Delegations — received from other agents */}
      {receivedTasks.length > 0 && (
        <div className="mt-8">
          <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2 mb-3">
            <ArrowLeft size={14} className="text-sera-accent" />
            Received Task Delegations
          </h3>
          <div className="space-y-2">
            {receivedTasks.map((t) => {
              const delegation = (t.context as Record<string, unknown>)?.delegation as
                | { fromAgent?: string; delegatedAt?: string }
                | undefined;
              return (
                <div key={t.id} className="sera-card-static p-3 flex items-start gap-3">
                  <ArrowRight size={13} className="text-sera-text-muted mt-0.5 flex-shrink-0" />
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 mb-0.5">
                      <span className="text-xs text-sera-text-dim">
                        from{' '}
                        <span className="text-sera-text font-medium">
                          {delegation?.fromAgent ?? 'unknown'}
                        </span>
                      </span>
                      <Badge
                        variant={
                          t.status === 'completed'
                            ? 'success'
                            : t.status === 'failed'
                              ? 'error'
                              : 'default'
                        }
                        className="text-[9px]"
                      >
                        {t.status}
                      </Badge>
                    </div>
                    <p className="text-xs text-sera-text truncate">{t.task}</p>
                  </div>
                  <span className="text-[10px] text-sera-text-dim flex-shrink-0">
                    {formatDistanceToNow(t.createdAt)}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
