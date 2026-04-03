import { useState, useId } from 'react';
import {
  Server,
  Trash2,
  RefreshCw,
  Wifi,
  WifiOff,
  Globe,
  HardDrive,
  Plus,
  Zap,
  Search,
  CheckCircle2,
  XCircle,
  Loader2,
} from 'lucide-react';
import { toast } from 'sonner';
import {
  useProviders,
  useDeleteProvider,
  useDynamicProviderStatuses,
  useProviderTemplates,
  useAddProvider,
  useDiscoverModels,
} from '@/hooks/useProviders';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';
import { Card, CardHeader, CardContent } from '@/components/ui/card';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogClose,
} from '@/components/ui/dialog';
import { Tooltip } from '@/components/ui/tooltip';
import type { ProviderConfig } from '@/lib/api/types';
import type { ProviderTemplate } from '@/lib/api/providers';
import { request } from '@/lib/api/client';

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

// ── Template Activation Dialog ──────────────────────────────────────────────

function ActivateDialog({
  template,
  onClose,
}: {
  template: ProviderTemplate | null;
  onClose: () => void;
}) {
  const apiKeyId = useId();
  const baseUrlId = useId();
  const [apiKey, setApiKey] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [selectedModels, setSelectedModels] = useState<Set<string>>(new Set());
  const addProvider = useAddProvider();

  if (!template) return null;

  const allSelected = selectedModels.size === template.models.length;

  const handleActivate = async () => {
    if (!apiKey && !template.baseUrl) {
      toast.error('API key is required');
      return;
    }
    const models = selectedModels.size > 0 ? Array.from(selectedModels) : template.models;
    let successCount = 0;
    for (const modelName of models) {
      try {
        await addProvider.mutateAsync({
          modelName,
          api: template.api,
          provider: template.provider,
          baseUrl: baseUrl || template.baseUrl,
          apiKey: apiKey || undefined,
          apiKeyEnvVar: template.apiKeyEnvVar,
        });
        successCount++;
      } catch {
        toast.error(`Failed to add ${modelName}`);
      }
    }
    if (successCount > 0) {
      toast.success(
        `Added ${successCount} model${successCount > 1 ? 's' : ''} from ${template.displayName}`
      );
    }
    onClose();
  };

  return (
    <Dialog open={!!template} onOpenChange={(o: boolean) => !o && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Activate {template.displayName}</DialogTitle>
          <DialogDescription>{template.description}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4 mt-2">
          <div>
            <label htmlFor={apiKeyId} className="text-xs text-sera-text-muted block mb-1">
              API Key ({template.apiKeyEnvVar})
            </label>
            <Input
              id={apiKeyId}
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={`Paste your ${template.provider} API key…`}
            />
          </div>
          {!template.baseUrl && (
            <div>
              <label htmlFor={baseUrlId} className="text-xs text-sera-text-muted block mb-1">
                Base URL (optional)
              </label>
              <Input
                id={baseUrlId}
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                placeholder="https://api.example.com/v1"
              />
            </div>
          )}
          <div>
            <div className="flex items-center justify-between mb-2">
              <label className="text-xs text-sera-text-muted">
                Models ({selectedModels.size || template.models.length} selected)
              </label>
              <button
                type="button"
                className="text-[10px] text-sera-accent hover:underline"
                onClick={() =>
                  setSelectedModels(allSelected ? new Set() : new Set(template.models))
                }
              >
                {allSelected ? 'Deselect all' : 'Select all'}
              </button>
            </div>
            <div className="max-h-40 overflow-y-auto space-y-1 border border-sera-border rounded-lg p-2">
              {template.models.map((m) => (
                <label
                  key={m}
                  className="flex items-center gap-2 text-xs text-sera-text cursor-pointer"
                >
                  <input
                    type="checkbox"
                    checked={selectedModels.has(m) || selectedModels.size === 0}
                    onChange={(e) => {
                      const next = new Set(
                        selectedModels.size === 0 ? template.models : selectedModels
                      );
                      if (e.target.checked) next.add(m);
                      else next.delete(m);
                      setSelectedModels(next);
                    }}
                    className="rounded"
                  />
                  <span className="font-mono">{m}</span>
                </label>
              ))}
            </div>
          </div>
          <div className="flex gap-3 justify-end">
            <DialogClose asChild>
              <Button variant="ghost" size="sm">
                Cancel
              </Button>
            </DialogClose>
            <Button
              size="sm"
              onClick={() => void handleActivate()}
              disabled={addProvider.isPending}
            >
              {addProvider.isPending ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                <Plus size={14} />
              )}
              Activate
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

// ── Test Connection Button ──────────────────────────────────────────────────

function TestConnectionButton({ modelName }: { modelName: string }) {
  const [status, setStatus] = useState<'idle' | 'testing' | 'ok' | 'fail'>('idle');

  const handleTest = async () => {
    setStatus('testing');
    try {
      await request<{ ok: boolean }>(`/providers/${encodeURIComponent(modelName)}/test`, {
        method: 'POST',
      });
      setStatus('ok');
    } catch {
      setStatus('fail');
    }
    setTimeout(() => setStatus('idle'), 4000);
  };

  if (status === 'testing')
    return (
      <div role="status" aria-label="Testing connection">
        <Loader2 size={12} className="animate-spin text-sera-text-muted" />
      </div>
    );
  if (status === 'ok')
    return (
      <div role="status" aria-label="Connection successful">
        <CheckCircle2 size={12} className="text-sera-success" />
      </div>
    );
  if (status === 'fail')
    return (
      <div role="status" aria-label="Connection failed">
        <XCircle size={12} className="text-sera-error" />
      </div>
    );
  return (
    <Tooltip content="Test connection">
      <button
        type="button"
        onClick={() => void handleTest()}
        className="p-1 text-sera-text-dim hover:text-sera-accent transition-colors"
        aria-label="Test connection"
      >
        <Zap size={12} />
      </button>
    </Tooltip>
  );
}

// ── Discover Models Button ──────────────────────────────────────────────────

function DiscoverButton({ providerName }: { providerName: string }) {
  const discover = useDiscoverModels();
  const addProvider = useAddProvider();
  const [models, setModels] = useState<string[] | null>(null);

  const handleDiscover = async () => {
    try {
      const result = await discover.mutateAsync(providerName);
      setModels(result.models);
      toast.success(`Found ${result.models.length} models`);
    } catch {
      toast.error('Discovery failed');
    }
  };

  const handleAddModel = async (modelName: string) => {
    try {
      await addProvider.mutateAsync({
        modelName,
        api: 'openai',
        provider: providerName,
      });
      toast.success(`Added ${modelName}`);
      setModels((prev) => prev?.filter((m) => m !== modelName) ?? null);
    } catch {
      toast.error(`Failed to add ${modelName}`);
    }
  };

  return (
    <div>
      <Button
        size="sm"
        variant="ghost"
        onClick={() => void handleDiscover()}
        disabled={discover.isPending}
        aria-label={discover.isPending ? 'Discovering models' : 'Discover models'}
      >
        {discover.isPending ? <Loader2 size={12} className="animate-spin" /> : <Search size={12} />}
        Discover
      </Button>
      {models && models.length > 0 && (
        <div className="mt-2 ml-4 space-y-1">
          {models.map((m) => (
            <div key={m} className="flex items-center gap-2 text-xs">
              <span className="font-mono text-sera-text-muted">{m}</span>
              <button
                type="button"
                onClick={() => void handleAddModel(m)}
                className="text-sera-accent hover:underline text-[10px]"
              >
                + Add
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ── Main Page ───────────────────────────────────────────────────────────────

export default function ProvidersPage() {
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
              <CardHeader className="px-4 py-3 border-b border-sera-border flex-row items-center gap-2">
                {providerIcon(providerName)}
                <span className="text-sm font-semibold text-sera-text capitalize">
                  {providerName}
                </span>
                <Badge variant="default" className="ml-auto">
                  {models.length} model{models.length !== 1 ? 's' : ''}
                </Badge>
                <DiscoverButton providerName={providerName} />
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
      <ActivateDialog template={activateTemplate} onClose={() => setActivateTemplate(null)} />

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
