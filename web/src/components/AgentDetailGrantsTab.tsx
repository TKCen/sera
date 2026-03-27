import { useState } from 'react';
import { Shield, Plus, AlertCircle, Check, X, Trash2 } from 'lucide-react';
import {
  useAgentGrants,
  useCreateGrant,
  useRevokeGrant,
  usePermissionRequests,
  useDecidePermission,
} from '@/hooks/useAgents';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { TabLoading } from '@/components/AgentDetailTabLoading';

export function AgentDetailGrantsTab({ id }: { id: string }) {
  const { data, isLoading } = useAgentGrants(id);
  const { data: pendingRequests } = usePermissionRequests(id);
  const createGrant = useCreateGrant();
  const revokeGrant = useRevokeGrant();
  const decidePermission = useDecidePermission();
  const [showAdd, setShowAdd] = useState(false);
  const [dimension, setDimension] = useState('filesystem');
  const [value, setValue] = useState('');
  const [grantType, setGrantType] = useState<'session' | 'persistent'>('persistent');

  if (isLoading) return <TabLoading />;

  const allGrants = [
    ...(data?.persistent ?? []).map((g) => ({ ...g, source: 'persistent' as const })),
    ...(data?.session ?? []).map((g) => ({ ...g, source: 'session' as const })),
  ];
  const activeGrants = allGrants.filter((g) => !g.revoked_at);
  const revokedGrants = allGrants.filter((g) => g.revoked_at);

  async function handleCreate() {
    if (!value.trim()) return;
    try {
      await createGrant.mutateAsync({
        id,
        params: { dimension, value: value.trim(), grantType },
      });
      setValue('');
      setShowAdd(false);
    } catch {
      // error handled by mutation
    }
  }

  return (
    <div className="p-6 space-y-4 max-w-3xl">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold text-sera-text flex items-center gap-2">
          <Shield size={14} className="text-sera-accent" />
          Capability Grants
        </h3>
        <Button size="sm" variant="outline" onClick={() => setShowAdd((p) => !p)}>
          <Plus size={13} /> Add Grant
        </Button>
      </div>

      {/* Add Grant form */}
      {showAdd && (
        <div className="sera-card-static p-4 space-y-3 border-sera-accent/30">
          <div className="grid grid-cols-3 gap-3">
            <div>
              <label className="block text-xs font-medium text-sera-text-muted mb-1">
                Dimension
              </label>
              <select
                value={dimension}
                onChange={(e) => setDimension(e.target.value)}
                className="sera-input text-xs"
              >
                <option value="filesystem">Filesystem</option>
                <option value="network">Network</option>
                <option value="exec.commands">Exec Commands</option>
              </select>
            </div>
            <div>
              <label className="block text-xs font-medium text-sera-text-muted mb-1">Value</label>
              <Input
                value={value}
                onChange={(e) => setValue(e.target.value)}
                placeholder={
                  dimension === 'filesystem'
                    ? '/path/to/directory'
                    : dimension === 'network'
                      ? 'api.example.com'
                      : 'git'
                }
                className="text-xs"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-sera-text-muted mb-1">Type</label>
              <select
                value={grantType}
                onChange={(e) => setGrantType(e.target.value as 'session' | 'persistent')}
                className="sera-input text-xs"
              >
                <option value="persistent">Persistent</option>
                <option value="session">Session</option>
              </select>
            </div>
          </div>
          <div className="flex gap-2">
            <Button
              size="sm"
              onClick={() => {
                void handleCreate();
              }}
              disabled={!value.trim() || createGrant.isPending}
            >
              {createGrant.isPending ? 'Creating…' : 'Create Grant'}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setShowAdd(false)}>
              Cancel
            </Button>
          </div>
        </div>
      )}

      {/* Pending permission requests */}
      {pendingRequests && pendingRequests.length > 0 && (
        <div className="space-y-2">
          <h4 className="text-xs font-semibold text-sera-warning flex items-center gap-1.5">
            <AlertCircle size={12} />
            {pendingRequests.length} Pending Request{pendingRequests.length > 1 ? 's' : ''}
          </h4>
          <div className="sera-card-static overflow-hidden border-sera-warning/30">
            {pendingRequests.map((req) => (
              <div
                key={req.requestId}
                className="flex items-center justify-between gap-3 px-3 py-2.5 border-b border-sera-border/50 last:border-0"
              >
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 text-xs">
                    <Badge variant="accent">{req.dimension}</Badge>
                    <span className="font-mono text-sera-text truncate">{req.value}</span>
                  </div>
                  {req.reason && (
                    <p className="text-[10px] text-sera-text-muted mt-0.5 truncate">{req.reason}</p>
                  )}
                </div>
                <div className="flex items-center gap-1.5 shrink-0">
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-7 text-xs gap-1 text-green-400 border-green-400/30 hover:bg-green-400/10"
                    disabled={decidePermission.isPending}
                    onClick={() => {
                      void decidePermission.mutateAsync({
                        requestId: req.requestId,
                        agentId: id,
                        params: { decision: 'grant', grantType: 'session' },
                      });
                    }}
                  >
                    <Check size={11} /> Grant
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-7 text-xs gap-1 text-sera-error border-sera-error/30 hover:bg-sera-error/10"
                    disabled={decidePermission.isPending}
                    onClick={() => {
                      void decidePermission.mutateAsync({
                        requestId: req.requestId,
                        agentId: id,
                        params: { decision: 'deny' },
                      });
                    }}
                  >
                    <X size={11} /> Deny
                  </Button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Active grants */}
      {activeGrants.length === 0 ? (
        <div className="sera-card-static p-8 text-center text-sm text-sera-text-muted">
          No active grants. Grants are created when agents request additional permissions or when
          operators add them manually.
        </div>
      ) : (
        <div className="sera-card-static overflow-hidden">
          <table className="w-full text-xs">
            <thead>
              <tr className="border-b border-sera-border text-[10px] uppercase tracking-wider text-sera-text-dim">
                <th className="text-left py-2.5 px-3">Dimension</th>
                <th className="text-left py-2.5 px-3">Value</th>
                <th className="text-left py-2.5 px-3">Type</th>
                <th className="text-left py-2.5 px-3">Granted</th>
                <th className="py-2.5 px-3" />
              </tr>
            </thead>
            <tbody>
              {activeGrants.map((g) => (
                <tr
                  key={g.id}
                  className="border-b border-sera-border/50 hover:bg-sera-surface-hover transition-colors"
                >
                  <td className="py-2.5 px-3">
                    <Badge variant="accent">{g.dimension}</Badge>
                  </td>
                  <td className="py-2.5 px-3 font-mono text-sera-text">{g.value}</td>
                  <td className="py-2.5 px-3">
                    <Badge variant={g.source === 'persistent' ? 'default' : 'warning'}>
                      {g.grant_type}
                    </Badge>
                  </td>
                  <td className="py-2.5 px-3 text-sera-text-muted">
                    {g.created_at ? new Date(g.created_at).toLocaleString() : '—'}
                  </td>
                  <td className="py-2.5 px-3 text-right">
                    <button
                      onClick={() => {
                        void revokeGrant.mutateAsync({ id, grantId: g.id });
                      }}
                      className="text-sera-text-dim hover:text-sera-error transition-colors p-1"
                      title="Revoke grant"
                    >
                      <Trash2 size={12} />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Revoked grants */}
      {revokedGrants.length > 0 && (
        <details className="text-xs">
          <summary className="text-sera-text-dim cursor-pointer hover:text-sera-text transition-colors py-2">
            {revokedGrants.length} revoked grant(s)
          </summary>
          <div className="sera-card-static overflow-hidden mt-2 opacity-60">
            <table className="w-full text-xs">
              <tbody>
                {revokedGrants.map((g) => (
                  <tr key={g.id} className="border-b border-sera-border/30">
                    <td className="py-2 px-3 text-sera-text-dim">{g.dimension}</td>
                    <td className="py-2 px-3 font-mono text-sera-text-dim line-through">
                      {g.value}
                    </td>
                    <td className="py-2 px-3 text-sera-text-dim">{g.grant_type}</td>
                    <td className="py-2 px-3 text-sera-text-dim">
                      Revoked {g.revoked_at ? new Date(g.revoked_at).toLocaleString() : ''}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </details>
      )}
    </div>
  );
}
