import { useState } from 'react';
import { useAgentDelegations, useAgentDelegatedTasks } from '@/hooks/useAgents';
import { useIssueDelegation } from '@/hooks/useDelegations';
import { TabLoading } from './AgentDetailTabLoading';
import { EmptyState } from './EmptyState';
import { Badge } from './ui/badge';
import { Button } from './ui/button';
import { Input } from './ui/input';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '@/components/ui/dialog';
import { toast } from 'sonner';
import { ShieldCheck, User, Clock, Activity, ArrowRight, ArrowLeft, Plus } from 'lucide-react';
import { formatDistanceToNow } from '@/lib/utils';

export function AgentDetailDelegationsTab({ id }: { id: string }) {
  const { data: delegations, isLoading, refetch } = useAgentDelegations(id);
  const { data: tasks } = useAgentDelegatedTasks(id);
  const issueDelegation = useIssueDelegation();

  const [showIssueDialog, setShowIssueDialog] = useState(false);
  const [newDelegation, setNewDelegation] = useState({
    service: '',
    permissions: '',
    credentialSecretName: '',
    grantType: 'session' as 'one-time' | 'session' | 'persistent',
    expiresAt: '',
  });

  const handleIssue = async () => {
    try {
      await issueDelegation.mutateAsync({
        agentId: id,
        service: newDelegation.service,
        permissions: newDelegation.permissions
          .split(',')
          .map((p) => p.trim())
          .filter(Boolean),
        credentialSecretName: newDelegation.credentialSecretName,
        grantType: newDelegation.grantType,
        expiresAt: newDelegation.expiresAt || undefined,
      });
      toast.success('Delegation issued');
      setShowIssueDialog(false);
      void refetch();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to issue delegation');
    }
  };

  // Separate delegated tasks: tasks received from other agents vs tasks this agent sent
  const receivedTasks = (tasks ?? []).filter((t) => {
    const ctx = t.context as Record<string, unknown> | null;
    return ctx?.delegation;
  });

  if (isLoading) return <TabLoading />;

  if (!delegations || delegations.length === 0) {
    return (
      <EmptyState
        icon={<ShieldCheck size={24} />}
        title="No inbound delegations"
        description="This agent hasn't received any delegated authority from operators or other agents yet."
      />
    );
  }

  return (
    <div className="p-6 space-y-6 max-w-5xl">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
          <ShieldCheck size={14} className="text-sera-accent" />
          Inbound Delegations
        </h3>
        <div className="flex items-center gap-3">
          <span className="text-xs text-sera-text-dim">{delegations.length} active</span>
          <Button size="sm" onClick={() => setShowIssueDialog(true)}>
            <Plus size={14} /> Issue Delegation
          </Button>
        </div>
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
      {/* Issue Delegation Dialog */}
      <Dialog open={showIssueDialog} onOpenChange={setShowIssueDialog}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Issue Delegation</DialogTitle>
            <DialogDescription>
              Grant this agent authority to act on your behalf for a specific service.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3 py-4">
            <div className="space-y-1.5">
              <label className="text-xs text-sera-text-muted">Service Name</label>
              <Input
                value={newDelegation.service}
                onChange={(e) => setNewDelegation({ ...newDelegation, service: e.target.value })}
                placeholder="e.g. github, slack, aws"
              />
            </div>
            <div className="space-y-1.5">
              <label className="text-xs text-sera-text-muted">Permissions (comma-separated)</label>
              <Input
                value={newDelegation.permissions}
                onChange={(e) =>
                  setNewDelegation({ ...newDelegation, permissions: e.target.value })
                }
                placeholder="e.g. repo:read, repo:write"
              />
            </div>
            <div className="space-y-1.5">
              <label className="text-xs text-sera-text-muted">Credential Secret Name</label>
              <Input
                value={newDelegation.credentialSecretName}
                onChange={(e) =>
                  setNewDelegation({ ...newDelegation, credentialSecretName: e.target.value })
                }
                placeholder="Name of the secret in the vault"
              />
            </div>
            <div className="flex gap-4">
              <div className="flex-1 space-y-1.5">
                <label className="text-xs text-sera-text-muted">Grant Type</label>
                <select
                  value={newDelegation.grantType}
                  onChange={(e) =>
                    setNewDelegation({
                      ...newDelegation,
                      grantType: e.target.value as 'one-time' | 'session' | 'persistent',
                    })
                  }
                  className="sera-input text-xs w-full"
                >
                  <option value="one-time">One-time</option>
                  <option value="session">Session</option>
                  <option value="persistent">Persistent</option>
                </select>
              </div>
              <div className="flex-1 space-y-1.5">
                <label className="text-xs text-sera-text-muted">Expires At (optional)</label>
                <Input
                  type="datetime-local"
                  value={newDelegation.expiresAt}
                  onChange={(e) =>
                    setNewDelegation({ ...newDelegation, expiresAt: e.target.value })
                  }
                  className="text-xs"
                />
              </div>
            </div>
          </div>
          <DialogFooter>
            <Button variant="ghost" size="sm" onClick={() => setShowIssueDialog(false)}>
              Cancel
            </Button>
            <Button size="sm" onClick={handleIssue} disabled={issueDelegation.isPending}>
              Issue
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
