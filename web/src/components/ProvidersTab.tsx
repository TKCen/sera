import { useState } from 'react';
import { Server, Trash2, RefreshCw, Wifi, WifiOff, Globe, HardDrive } from 'lucide-react';
import { toast } from 'sonner';
import {
  useProviders,
  useDeleteProvider,
  useDynamicProviderStatuses,
  useProviderTemplates,
} from '@/hooks/useProviders';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/card';
import { Tooltip } from '@/components/ui/tooltip';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogClose,
} from '@/components/ui/dialog';
import { ProviderActivateDialog } from '@/components/providers/ProviderActivateDialog';
import { TestConnectionButton } from '@/components/providers/TestConnectionButton';
import { DiscoverModelsButton } from '@/components/providers/DiscoverModelsButton';
import type { ProviderConfig } from '@/lib/api/types';
import type { ProviderTemplate } from '@/lib/api/providers';

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

export function ProvidersTab() {
  const { data, isLoading, refetch } = useProviders();
  const { data: statuses } = useDynamicProviderStatuses();
  const { data: templateData } = useProviderTemplates();
  const deleteProvider = useDeleteProvider();
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [activateTemplate, setActivateTemplate] = useState<ProviderTemplate | null>(null);

  const providers = data?.providers ?? [];
  const grouped = groupByProvider(providers);
  const templates = templateData?.templates ?? [];

  // Filter out templates that are already fully activated
  const activeProviderNames = new Set(providers.map((p) => p.provider));
  const availableTemplates = templates.filter((t) => !activeProviderNames.has(t.provider));

  const handleDelete = async (name: string) => {
    try {
      await deleteProvider.mutateAsync(name);
      toast.success(`Provider "${name}" deleted`);
    } catch {
      toast.error('Failed to delete provider');
    }
    setConfirmDelete(null);
  };

  const statusMap = new Map<string, string>();
  if (statuses && Array.isArray(statuses)) {
    for (const s of statuses) {
      if (s && typeof s === 'object' && 'id' in s && 'status' in s) {
        statusMap.set(String(s.id), String(s.status));
      }
    }
  }

  return (
    <div className="space-y-6 animate-in fade-in slide-in-from-bottom-2 duration-300">
      <div className="flex items-center justify-between">
        <div>
          <p className="text-sm text-sera-text-muted">
            {providers.length} model{providers.length !== 1 ? 's' : ''} configured
          </p>
        </div>
        <Tooltip content="Refresh providers">
          <Button size="sm" variant="outline" onClick={() => void refetch()}>
            <RefreshCw size={13} /> Refresh
          </Button>
        </Tooltip>
      </div>

      {/* ── Available Templates ──────────────────────────────────────────── */}
      {availableTemplates.length > 0 && (
        <section>
          <h2 className="text-sm font-semibold text-sera-text mb-3">Available Providers</h2>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
            {availableTemplates.map((t) => (
              <button
                key={t.provider}
                onClick={() => setActivateTemplate(t)}
                className="sera-card-static p-4 text-left hover:border-sera-accent/50 transition-colors group"
              >
                <div className="flex items-center gap-2 mb-1">
                  {providerIcon(t.provider)}
                  <span className="text-sm font-medium text-sera-text">{t.displayName}</span>
                </div>
                <p className="text-xs text-sera-text-muted line-clamp-2">{t.description}</p>
                <div className="flex items-center gap-2 mt-2">
                  <Badge variant="default" className="text-[10px]">
                    {t.models.length} models
                  </Badge>
                  <span className="text-[10px] text-sera-accent opacity-0 group-hover:opacity-100 transition-opacity">
                    Click to activate
                  </span>
                </div>
              </button>
            ))}
          </div>
        </section>
      )}

      {/* ── Active Providers ─────────────────────────────────────────────── */}
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
          description="Activate a provider above or configure in core/config/providers.json."
        />
      ) : (
        <div className="space-y-6">
          {Object.entries(grouped).map(([providerName, models]) => (
            <Card key={providerName} className="p-0 overflow-hidden">
              <CardHeader className="px-4 py-3 border-b border-sera-border flex-row items-center gap-2 space-y-0">
                {providerIcon(providerName)}
                <CardTitle className="text-sm font-semibold text-sera-text capitalize">
                  {providerName}
                </CardTitle>
                <Badge variant="default" className="ml-auto">
                  {models.length} model{models.length !== 1 ? 's' : ''}
                </Badge>
                <DiscoverModelsButton providerName={providerName} />
              </CardHeader>
              <CardContent className="divide-y divide-sera-border/50">
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
                          <div className="flex flex-wrap gap-x-3 gap-y-1 mt-1.5">
                            {m.contextWindow && (
                              <span className="text-[10px] text-sera-text-dim font-medium bg-sera-surface-active/50 px-1.5 py-0.5 rounded border border-sera-border/30">
                                ctx: {(m.contextWindow / 1024).toFixed(0)}K
                              </span>
                            )}
                            {m.maxTokens && (
                              <span className="text-[10px] text-sera-text-dim font-medium bg-sera-surface-active/50 px-1.5 py-0.5 rounded border border-sera-border/30">
                                max: {(m.maxTokens / 1024).toFixed(1)}K
                              </span>
                            )}
                            {m.contextStrategy && (
                              <span className="text-[10px] text-sera-text-dim font-medium bg-sera-surface-active/50 px-1.5 py-0.5 rounded border border-sera-border/30">
                                strategy: {m.contextStrategy}
                              </span>
                            )}
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
                        <TestConnectionButton modelName={m.modelName} />
                        {!isDynamic && (
                          <Tooltip content="Delete provider">
                            <button
                              type="button"
                              onClick={() => setConfirmDelete(m.modelName)}
                              className="p-1 text-sera-text-dim hover:text-sera-error transition-colors"
                              aria-label="Delete provider"
                            >
                              <Trash2 size={12} />
                            </button>
                          </Tooltip>
                        )}
                      </div>
                    </div>
                  );
                })}
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {/* ── Activate Dialog ──────────────────────────────────────────────── */}
      <ProviderActivateDialog
        template={activateTemplate}
        onClose={() => setActivateTemplate(null)}
      />

      {/* ── Delete Confirmation ──────────────────────────────────────────── */}
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
