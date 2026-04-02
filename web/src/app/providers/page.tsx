import { useState } from 'react';
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
  Radio,
  Save,
  Cloud,
  Key,
  ChevronDown,
  ChevronUp,
} from 'lucide-react';
import { toast } from 'sonner';
import {
  useProviders,
  useDeleteProvider,
  useDynamicProviders,
  useDynamicProviderStatuses,
  useProviderTemplates,
  useAddProvider,
  useDiscoverModels,
  useAddDynamicProvider,
  useRemoveDynamicProvider,
} from '@/hooks/useProviders';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
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
import type { ProviderTemplate, AddProviderPayload } from '@/lib/api/providers';
import { request } from '@/lib/api/client';
import * as providersApi from '@/lib/api/providers';
import { DynamicProviderCard } from '@/components/DynamicProviderCard';

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
    return <Loader2 size={12} className="animate-spin text-sera-text-muted" />;
  if (status === 'ok') return <CheckCircle2 size={12} className="text-sera-success" />;
  if (status === 'fail') return <XCircle size={12} className="text-sera-error" />;
  return (
    <button
      onClick={() => void handleTest()}
      className="p-1 text-sera-text-dim hover:text-sera-accent transition-colors"
      title="Test connection"
    >
      <Zap size={12} />
    </button>
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

// ── Cloud Template Card ─────────────────────────────────────────────────────

function AuthBadge({ status }: { status: string }) {
  if (status === 'configured')
    return (
      <Badge variant="success" className="text-[9px] px-1.5 py-0">
        Configured
      </Badge>
    );
  if (status === 'not-required')
    return (
      <Badge variant="default" className="text-[9px] px-1.5 py-0">
        No auth
      </Badge>
    );
  return (
    <Badge variant="warning" className="text-[9px] px-1.5 py-0">
      Key missing
    </Badge>
  );
}

function TemplateCard({
  template,
  activeModels,
  onActivate,
  onRemoveModel,
}: {
  template: ProviderTemplate;
  activeModels: Array<{ modelName: string; authStatus?: string }>;
  onActivate: (payloads: AddProviderPayload[]) => Promise<void>;
  onRemoveModel: (modelName: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [apiKey, setApiKey] = useState('');
  const [customModel, setCustomModel] = useState('');
  const [saving, setSaving] = useState(false);
  const discover = useDiscoverModels();

  const isActive = activeModels.length > 0;
  const isConfigured = activeModels.some((m) => m.authStatus === 'configured');

  const handleActivate = async () => {
    if (!apiKey) return;
    setSaving(true);
    try {
      const payloads: AddProviderPayload[] = template.models.map((modelName) => ({
        modelName,
        api: template.api,
        provider: template.provider,
        apiKey,
        ...(template.baseUrl ? { baseUrl: template.baseUrl } : {}),
        description: `${template.displayName} — ${modelName}`,
      }));
      await onActivate(payloads);
      setApiKey('');
    } finally {
      setSaving(false);
    }
  };

  const handleAddCustomModel = async () => {
    if (!customModel.trim()) return;
    const payload: AddProviderPayload = {
      modelName: customModel.trim(),
      api: template.api,
      provider: template.provider,
      ...(template.baseUrl ? { baseUrl: template.baseUrl } : {}),
      description: `${template.displayName} — ${customModel.trim()}`,
    };
    await onActivate([payload]);
    setCustomModel('');
  };

  const handleDiscover = () => {
    if (activeModels.length > 0) {
      discover.mutate(activeModels[0].modelName);
    }
  };

  return (
    <div
      className={`sera-card-static overflow-hidden transition-colors ${
        isActive
          ? isConfigured
            ? 'border-sera-success/30'
            : 'border-amber-500/30'
          : 'border-sera-border'
      }`}
    >
      <button
        onClick={() => setExpanded((e) => !e)}
        className="w-full p-4 flex items-center justify-between hover:bg-sera-surface-hover transition-colors"
      >
        <div className="flex items-center gap-3">
          <div
            className={`w-9 h-9 rounded-lg flex items-center justify-center border ${
              isActive
                ? 'bg-sera-success/10 border-sera-success/20'
                : 'bg-sera-surface border-sera-border'
            }`}
          >
            <Cloud size={16} className={isActive ? 'text-sera-success' : 'text-sera-text-dim'} />
          </div>
          <div className="text-left">
            <div className="flex items-center gap-2">
              <h3 className="text-sm font-semibold text-sera-text">{template.displayName}</h3>
              {isActive ? (
                <AuthBadge status={activeModels[0]?.authStatus ?? 'missing'} />
              ) : (
                <Badge variant="default" className="text-[9px] px-1.5 py-0">
                  Not configured
                </Badge>
              )}
            </div>
            <p className="text-[11px] text-sera-text-muted mt-0.5">{template.description}</p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          {isActive && (
            <span className="text-[11px] text-sera-text-dim px-2 py-0.5 rounded-full bg-sera-bg/80 border border-sera-border">
              {activeModels.length} model{activeModels.length !== 1 ? 's' : ''}
            </span>
          )}
          {expanded ? (
            <ChevronUp size={14} className="text-sera-text-dim" />
          ) : (
            <ChevronDown size={14} className="text-sera-text-dim" />
          )}
        </div>
      </button>

      {expanded && (
        <div className="border-t border-sera-border p-4 space-y-4 bg-sera-bg/50 animate-in slide-in-from-top-2 duration-200">
          {/* Active models */}
          {activeModels.length > 0 && (
            <div className="space-y-2">
              <span className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                Active Models
              </span>
              <div className="space-y-1">
                {activeModels.map((m) => (
                  <div
                    key={m.modelName}
                    className="flex items-center justify-between px-3 py-2 rounded-lg bg-sera-surface/50 border border-sera-border/50"
                  >
                    <span className="font-mono text-xs text-sera-text">{m.modelName}</span>
                    <button
                      onClick={() => onRemoveModel(m.modelName)}
                      className="text-sera-text-dim hover:text-sera-error transition-colors"
                    >
                      <Trash2 size={12} />
                    </button>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* API Key input (when not yet configured) */}
          {!isActive && (
            <div className="space-y-3">
              <div className="space-y-1.5">
                <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider flex items-center gap-1.5">
                  <Key size={10} /> API Key
                </label>
                <input
                  type="password"
                  placeholder={`Enter your ${template.displayName} API key`}
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
                  className="sera-input text-xs"
                />
              </div>
              <div className="flex items-center gap-2 text-[10px] text-sera-text-dim">
                <span>Will register: {template.models.join(', ')}</span>
              </div>
              <Button
                className="w-full text-xs bg-sera-accent hover:bg-sera-accent-hover text-sera-bg h-9"
                disabled={!apiKey || saving}
                onClick={handleActivate}
              >
                {saving ? (
                  <Loader2 size={13} className="animate-spin" />
                ) : (
                  <CheckCircle2 size={13} />
                )}
                Activate {template.displayName}
              </Button>
            </div>
          )}

          {/* Add custom model + discover */}
          {isActive && (
            <div className="space-y-3 border-t border-sera-border pt-3">
              <div className="flex gap-2">
                <div className="flex-1 space-y-1.5">
                  <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                    Add model by name
                  </label>
                  <div className="flex gap-2">
                    <input
                      type="text"
                      placeholder="e.g. gemini-3-flash-preview"
                      value={customModel}
                      onChange={(e) => setCustomModel(e.target.value)}
                      className="sera-input text-xs font-mono flex-1"
                    />
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-9 text-xs gap-1"
                      disabled={!customModel.trim()}
                      onClick={handleAddCustomModel}
                    >
                      <Plus size={12} /> Add
                    </Button>
                  </div>
                </div>
              </div>

              {template.supportsDiscovery && (
                <Button
                  variant="outline"
                  size="sm"
                  className="w-full text-xs h-9 gap-1.5"
                  disabled={discover.isPending}
                  onClick={handleDiscover}
                >
                  {discover.isPending ? (
                    <Loader2 size={13} className="animate-spin" />
                  ) : (
                    <Search size={13} />
                  )}
                  Discover Available Models
                </Button>
              )}

              {discover.data && (
                <div className="space-y-2">
                  <span className="text-[11px] text-sera-text-dim">
                    Found {discover.data.models.length} model(s):
                  </span>
                  <div className="flex flex-wrap gap-1.5 p-2 rounded-lg bg-sera-bg/30 border border-sera-border/50 max-h-40 overflow-y-auto">
                    {discover.data.models.map((m) => {
                      const isRegistered = activeModels.some((a) => a.modelName === m);
                      return (
                        <button
                          key={m}
                          disabled={isRegistered}
                          onClick={() => {
                            setCustomModel(m);
                          }}
                          className={`px-2 py-1 rounded border text-[10px] font-mono transition-colors ${
                            isRegistered
                              ? 'border-sera-success/30 bg-sera-success/5 text-sera-success cursor-default'
                              : 'border-sera-border bg-sera-surface/50 text-sera-text-muted hover:border-sera-accent/30 hover:text-sera-text cursor-pointer'
                          }`}
                        >
                          {isRegistered && <CheckCircle2 size={8} className="inline mr-1" />}
                          {m}
                        </button>
                      );
                    })}
                  </div>
                </div>
              )}

              {discover.error && (
                <div className="flex items-start gap-2 p-3 rounded-lg bg-sera-error/5 border border-sera-error/20 text-sera-error text-[11px]">
                  <XCircle size={14} className="mt-0.5 shrink-0" />
                  <span>
                    Discovery failed:{' '}
                    {discover.error instanceof Error
                      ? discover.error.message
                      : String(discover.error)}
                  </span>
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ── Main Page ───────────────────────────────────────────────────────────────

export default function ProvidersPage() {
  const { data, isLoading, refetch: refetchProviders } = useProviders();
  const { data: dynamicData, isLoading: isLoadingDynamic } = useDynamicProviders();
  const { data: statusesData } = useDynamicProviderStatuses();
  const { data: templateData } = useProviderTemplates();
  const addProviderMut = useAddProvider();
  const deleteProviderMut = useDeleteProvider();
  const addDynamic = useAddDynamicProvider();
  const removeDynamic = useRemoveDynamicProvider();

  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [showAddDynamic, setShowAddDynamic] = useState(false);
  const [newDynamic, setNewDynamic] = useState({
    id: '',
    name: '',
    baseUrl: 'http://host.docker.internal:1234/v1',
    apiKey: '',
  });
  const [testResult, setTestResult] = useState<{
    success: boolean;
    models: string[];
    error?: string;
  } | null>(null);
  const [isTesting, setIsTesting] = useState(false);

  const providers = data?.providers ?? [];
  const grouped = groupByProvider(providers);
  const templates = templateData?.templates ?? [];

  const handleActivate = async (payloads: AddProviderPayload[]) => {
    for (const payload of payloads) {
      await addProviderMut.mutateAsync(payload);
    }
    void refetchProviders();
  };

  const handleRemoveModel = async (modelName: string) => {
    await deleteProviderMut.mutateAsync(modelName);
    void refetchProviders();
  };

  const handleDelete = async (name: string) => {
    try {
      await deleteProviderMut.mutateAsync(name);
      toast.success(`Provider "${name}" deleted`);
    } catch {
      toast.error('Failed to delete provider');
    }
    setConfirmDelete(null);
  };

  const statusMap = new Map<string, string>();
  if (statusesData && Array.isArray(statusesData.statuses)) {
    for (const s of statusesData.statuses) {
      statusMap.set(s.id, s.status);
    }
  }

  return (
    <div className="p-8 max-w-7xl mx-auto space-y-10">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Providers</h1>
          <p className="text-sm text-sera-text-muted mt-1">
            Manage model providers, cloud APIs, and dynamic discovery
          </p>
        </div>
        <Button size="sm" variant="outline" onClick={() => void refetchProviders()}>
          <RefreshCw size={13} /> Refresh
        </Button>
      </div>

      {/* ── Dynamic Discovery Section ────────────────────────────────────── */}
      <section className="space-y-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Radio size={14} className="text-amber-400" />
            <h2 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase">
              Dynamic Discovery
            </h2>
            <span className="text-[11px] text-sera-text-dim/60">— LM Studio, Ollama, etc.</span>
          </div>
          <Button
            size="sm"
            variant="outline"
            className="h-8 text-[11px] gap-1.5"
            onClick={() => setShowAddDynamic(true)}
          >
            <Plus size={14} /> Add Provider
          </Button>
        </div>

        {showAddDynamic && (
          <div className="sera-card-static p-5 border-sera-accent/30 bg-sera-accent/5 animate-in zoom-in-95 duration-200">
            <div className="flex justify-between items-start mb-4">
              <h3 className="text-sm font-semibold text-sera-text">Add LM Studio Instance</h3>
              <button
                onClick={() => setShowAddDynamic(false)}
                className="text-sera-text-dim hover:text-sera-text"
              >
                <XCircle size={16} />
              </button>
            </div>

            <div className="grid grid-cols-2 gap-4 mb-4">
              <div className="space-y-1.5">
                <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                  Provider Name
                </label>
                <Input
                  placeholder="e.g. Local LM Studio"
                  value={newDynamic.name}
                  onChange={(e) => setNewDynamic({ ...newDynamic, name: e.target.value })}
                  className="text-xs"
                />
              </div>
              <div className="space-y-1.5">
                <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                  Unique ID
                </label>
                <Input
                  placeholder="e.g. lmstudio-1"
                  value={newDynamic.id}
                  onChange={(e) =>
                    setNewDynamic({
                      ...newDynamic,
                      id: e.target.value.toLowerCase().replace(/\s+/g, '-'),
                    })
                  }
                  className="text-xs font-mono"
                />
              </div>
              <div className="space-y-1.5 col-span-2">
                <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                  Base URL (with /v1)
                </label>
                <Input
                  value={newDynamic.baseUrl}
                  onChange={(e) => setNewDynamic({ ...newDynamic, baseUrl: e.target.value })}
                  className="text-xs font-mono"
                />
                <p className="text-[10px] text-sera-text-dim mt-0.5">
                  Running in Docker? Use <code className="font-mono">host.docker.internal</code>{' '}
                  instead of <code className="font-mono">localhost</code>
                </p>
              </div>
              <div className="space-y-1.5 col-span-2">
                <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                  API Key <span className="text-sera-text-dim/50">(optional)</span>
                </label>
                <Input
                  type="password"
                  value={newDynamic.apiKey}
                  onChange={(e) => setNewDynamic({ ...newDynamic, apiKey: e.target.value })}
                  className="text-xs"
                />
              </div>
            </div>

            {testResult && (
              <div
                className={`mb-4 overflow-hidden rounded-lg border text-xs ${
                  testResult.success
                    ? 'bg-sera-success/10 border-sera-success/20 text-sera-success'
                    : 'bg-sera-error/10 border-sera-error/20 text-sera-error'
                }`}
              >
                <div className="p-3 flex items-start gap-2">
                  {testResult.success ? <CheckCircle2 size={14} /> : <XCircle size={14} />}
                  <div>
                    <p className="font-semibold">
                      {testResult.success ? 'Connection successful' : 'Connection failed'}
                    </p>
                    {!testResult.success && <p className="mt-0.5 opacity-90">{testResult.error}</p>}
                    {testResult.success && (
                      <p className="mt-1 opacity-90">
                        Found {testResult.models.length} model(s): {testResult.models.join(', ')}
                      </p>
                    )}
                  </div>
                </div>
              </div>
            )}

            <div className="flex gap-3">
              <Button
                variant="outline"
                className="flex-1 text-xs h-10"
                disabled={isTesting || !newDynamic.baseUrl}
                onClick={async () => {
                  setIsTesting(true);
                  setTestResult(null);
                  try {
                    const res = await providersApi.testDynamicConnection(
                      newDynamic.baseUrl,
                      newDynamic.apiKey
                    );
                    setTestResult(res);
                  } catch (err: unknown) {
                    setTestResult({
                      success: false,
                      models: [],
                      error: err instanceof Error ? err.message : String(err),
                    });
                  } finally {
                    setIsTesting(false);
                  }
                }}
              >
                {isTesting ? <Loader2 className="animate-spin" size={14} /> : <Zap size={14} />}
                Test & Discover
              </Button>
              <Button
                className="flex-1 text-xs h-10"
                disabled={
                  !(
                    newDynamic.name &&
                    newDynamic.id &&
                    newDynamic.baseUrl &&
                    testResult?.success &&
                    !addDynamic.isPending
                  )
                }
                onClick={() => {
                  addDynamic.mutate({
                    ...newDynamic,
                    type: 'lm-studio',
                    enabled: true,
                    intervalMs: 60000,
                  });
                  setShowAddDynamic(false);
                  setNewDynamic({
                    id: '',
                    name: '',
                    baseUrl: 'http://host.docker.internal:1234/v1',
                    apiKey: '',
                  });
                  setTestResult(null);
                }}
              >
                <Save size={14} /> Save Provider
              </Button>
            </div>
          </div>
        )}

        <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
          {(dynamicData?.dynamicProviders ?? []).map((p) => (
            <DynamicProviderCard
              key={p.id}
              provider={p}
              status={statusesData?.statuses.find((s) => s.id === p.id)}
              onRemove={(id) => removeDynamic.mutate(id)}
            />
          ))}
          {!isLoadingDynamic &&
            (dynamicData?.dynamicProviders ?? []).length === 0 &&
            !showAddDynamic && (
              <div className="col-span-full sera-card-static border-dashed border-sera-border p-8 text-center">
                <p className="text-xs text-sera-text-muted mb-3">
                  No dynamic providers configured.
                </p>
                <Button size="sm" variant="outline" onClick={() => setShowAddDynamic(true)}>
                  <Plus size={12} /> Configure Local Provider
                </Button>
              </div>
            )}
        </div>
      </section>

      {/* ── Cloud Providers Section ──────────────────────────────────────── */}
      <section className="space-y-4">
        <div className="flex items-center gap-2">
          <Cloud size={14} className="text-sky-400" />
          <h2 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase">
            Cloud Providers
          </h2>
          <span className="text-[11px] text-sera-text-dim/60">
            — OpenAI, Anthropic, Google, etc.
          </span>
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
          {templates.map((t) => (
            <TemplateCard
              key={t.provider}
              template={t}
              activeModels={providers.filter((p) => p.provider === t.provider)}
              onActivate={handleActivate}
              onRemoveModel={handleRemoveModel}
            />
          ))}
        </div>
      </section>

      {/* ── All Configured Models ────────────────────────────────────────── */}
      <section className="space-y-4">
        <div className="flex items-center gap-2">
          <Server size={14} className="text-sera-text-muted" />
          <h2 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase">
            All Configured Models
          </h2>
          <Badge variant="default" className="text-[10px]">
            {providers.length} total
          </Badge>
        </div>

        {isLoading ? (
          <div className="space-y-4">
            {[1, 2].map((i) => (
              <Skeleton key={i} className="h-20 rounded-xl" />
            ))}
          </div>
        ) : providers.length === 0 ? (
          <EmptyState
            icon={<Server size={24} />}
            title="No models configured"
            description="Activate a cloud provider or add a dynamic discovery source above."
          />
        ) : (
          <div className="space-y-6">
            {Object.entries(grouped).map(([providerName, models]) => (
              <div key={providerName} className="sera-card-static overflow-hidden">
                <div className="px-4 py-3 border-b border-sera-border flex items-center gap-2 bg-sera-surface/30">
                  {providerIcon(providerName)}
                  <span className="text-sm font-semibold text-sera-text capitalize">
                    {providerName}
                  </span>
                  <Badge variant="default" className="ml-auto">
                    {models.length} model{models.length !== 1 ? 's' : ''}
                  </Badge>
                  <DiscoverButton providerName={providerName} />
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
                          <TestConnectionButton modelName={m.modelName} />
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
      </section>

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
              disabled={deleteProviderMut.isPending}
            >
              Delete
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
