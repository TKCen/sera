import { useState } from 'react';
import {
  Cloud,
  Key,
  ChevronDown,
  ChevronUp,
  CheckCircle,
  XCircle,
  Search,
  Plus,
  Trash2,
  Loader2,
} from 'lucide-react';
import {
  useProviderTemplates,
  useProviders,
  useAddProvider,
  useDeleteProvider,
  useDiscoverModels,
} from '@/hooks/useProviders';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import type { ProviderTemplate, AddProviderPayload } from '@/lib/api/providers';

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
  onActivate: (models: AddProviderPayload[]) => Promise<void>;
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
                <span>
                  Will register: {template.models.join(', ')}
                </span>
              </div>
              <Button
                className="w-full text-xs bg-sera-accent hover:bg-sera-accent-hover text-sera-bg h-9"
                disabled={!apiKey || saving}
                onClick={handleActivate}
              >
                {saving ? (
                  <Loader2 size={13} className="animate-spin" />
                ) : (
                  <CheckCircle size={13} />
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
                          {isRegistered && <CheckCircle size={8} className="inline mr-1" />}
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
                    {discover.error instanceof Error ? discover.error.message : String(discover.error)}
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

export function CloudProviderSection() {
  const { data: templatesData } = useProviderTemplates();
  const { data: providersData, refetch: refetchProviders } = useProviders();
  const addProviderMut = useAddProvider();
  const deleteProviderMut = useDeleteProvider();

  const templates = templatesData?.templates ?? [];
  const activeProviders = (providersData?.providers ?? []) as Array<{
    modelName: string;
    provider?: string;
    authStatus?: string;
  }>;

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

  return (
    <section>
      <div className="flex items-center gap-2 mb-4">
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
            activeModels={activeProviders.filter((p) => p.provider === t.provider)}
            onActivate={handleActivate}
            onRemoveModel={handleRemoveModel}
          />
        ))}
      </div>
    </section>
  );
}
