import { useState } from 'react';
import { Server, Trash2, RefreshCw, Wifi, WifiOff, Globe, HardDrive } from 'lucide-react';
import { toast } from 'sonner';
import { useProviders, useDeleteProvider, useDynamicProviderStatuses } from '@/hooks/useProviders';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogClose,
} from '@/components/ui/dialog';
import type { ProviderConfig } from '@/lib/api/types';

function providerIcon(provider?: string) {
  if (!provider) return <Server size={14} className="text-sera-text-muted" />;
  if (provider === 'ollama' || provider === 'lmstudio')
    return <HardDrive size={14} className="text-sera-accent" />;
  return <Globe size={14} className="text-sera-success" />;
}

function groupByProvider(providers: ProviderConfig[]) {
  const groups: Record<string, ProviderConfig[]> = {};
  for (const p of providers) {
    const key = p.provider ?? 'unknown';
    if (!groups[key]) groups[key] = [];
    groups[key]!.push(p);
  }
  return groups;
}

export default function ProvidersPage() {
  const { data, isLoading, refetch } = useProviders();
  const { data: statuses } = useDynamicProviderStatuses();
  const deleteProvider = useDeleteProvider();
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  const providers = data?.providers ?? [];
  const grouped = groupByProvider(providers);

  const handleDelete = async (name: string) => {
    try {
      await deleteProvider.mutateAsync(name);
      toast.success(`Provider "${name}" deleted`);
    } catch {
      toast.error('Failed to delete provider');
    }
    setConfirmDelete(null);
  };

  // Build a status lookup from dynamic provider statuses
  const statusMap = new Map<string, string>();
  if (statuses && Array.isArray(statuses)) {
    for (const s of statuses as Array<{ id: string; status: string }>) {
      statusMap.set(s.id, s.status);
    }
  }

  return (
    <div className="p-8 max-w-7xl mx-auto space-y-6">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Providers</h1>
          <p className="text-sm text-sera-text-muted mt-1">
            {providers.length} model{providers.length !== 1 ? 's' : ''} configured
          </p>
        </div>
        <Button size="sm" variant="outline" onClick={() => void refetch()}>
          <RefreshCw size={13} /> Refresh
        </Button>
      </div>

      {isLoading ? (
        <div className="space-y-4">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-20 rounded-xl" />
          ))}
        </div>
      ) : providers.length === 0 ? (
        <EmptyState
          icon={<Server size={24} />}
          title="No providers"
          description="Configure providers in core/config/providers.json or via environment variables."
        />
      ) : (
        <div className="space-y-6">
          {Object.entries(grouped).map(([providerName, models]) => (
            <div key={providerName} className="sera-card-static overflow-hidden">
              <div className="px-4 py-3 border-b border-sera-border flex items-center gap-2">
                {providerIcon(providerName)}
                <span className="text-sm font-semibold text-sera-text capitalize">
                  {providerName}
                </span>
                <Badge variant="default" className="ml-auto">
                  {models.length} model{models.length !== 1 ? 's' : ''}
                </Badge>
              </div>
              <div className="divide-y divide-sera-border/50">
                {models.map((m) => {
                  const isDynamic = !!m.dynamicProviderId;
                  const dpStatus = m.dynamicProviderId
                    ? statusMap.get(m.dynamicProviderId)
                    : undefined;

                  return (
                    <div
                      key={m.modelName}
                      className="px-4 py-3 flex items-center gap-4 hover:bg-sera-surface-hover/30 transition-colors"
                    >
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="text-sm font-medium text-sera-text font-mono">
                            {m.modelName}
                          </span>
                          {isDynamic && (
                            <Badge variant="accent" className="text-[10px]">
                              dynamic
                            </Badge>
                          )}
                        </div>
                        {m.description && (
                          <p className="text-xs text-sera-text-muted mt-0.5 truncate">
                            {m.description}
                          </p>
                        )}
                        {(m.contextWindow || m.maxTokens) && (
                          <div className="flex gap-2 mt-1 text-[10px] text-sera-text-dim">
                            {m.contextWindow && (
                              <span>ctx: {(m.contextWindow / 1024).toFixed(0)}K</span>
                            )}
                            {m.maxTokens && <span>max: {(m.maxTokens / 1024).toFixed(1)}K</span>}
                            {m.contextStrategy && <span>strategy: {m.contextStrategy}</span>}
                          </div>
                        )}
                      </div>

                      <div className="flex items-center gap-3 flex-shrink-0">
                        <span className="text-[10px] text-sera-text-dim font-mono">{m.api}</span>
                        {m.baseUrl && (
                          <span
                            className="text-[10px] text-sera-text-dim max-w-[200px] truncate"
                            title={m.baseUrl}
                          >
                            {m.baseUrl}
                          </span>
                        )}
                        {m.authStatus && (
                          <Badge
                            variant={
                              m.authStatus === 'configured'
                                ? 'success'
                                : m.authStatus === 'not-required'
                                  ? 'default'
                                  : 'warning'
                            }
                            className="text-[9px]"
                          >
                            {m.authStatus}
                          </Badge>
                        )}
                        {dpStatus && (
                          <span className="flex items-center gap-1 text-[10px]">
                            {dpStatus === 'connected' ? (
                              <Wifi size={10} className="text-sera-success" />
                            ) : (
                              <WifiOff size={10} className="text-sera-error" />
                            )}
                            {dpStatus}
                          </span>
                        )}
                        {!isDynamic && (
                          <button
                            onClick={() => setConfirmDelete(m.modelName)}
                            className="p-1 text-sera-text-dim hover:text-sera-error transition-colors"
                            title="Delete provider"
                          >
                            <Trash2 size={12} />
                          </button>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      )}

      <Dialog open={!!confirmDelete} onOpenChange={(o: boolean) => !o && setConfirmDelete(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete provider</DialogTitle>
            <DialogDescription>
              Remove <strong className="font-mono">{confirmDelete}</strong> from the provider
              registry? This cannot be undone.
            </DialogDescription>
          </DialogHeader>
          <div className="flex gap-3 justify-end mt-4">
            <DialogClose asChild>
              <Button variant="ghost" size="sm">
                Cancel
              </Button>
            </DialogClose>
            <Button
              size="sm"
              variant="danger"
              onClick={() => confirmDelete && void handleDelete(confirmDelete)}
              disabled={deleteProvider.isPending}
            >
              Delete
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
