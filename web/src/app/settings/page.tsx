import { useState, useEffect } from 'react';
import {
  Zap,
  CheckCircle,
  XCircle,
  Settings2,
  Sliders,
  Activity,
  Save,
  Search,
  AlertTriangle,
  ChevronDown,
  Brain,
} from 'lucide-react';
import { toast } from 'sonner';
import { updateProviderConfig } from '@/lib/api/providers';
import { useProviders, useLLMConfig } from '@/hooks/useProviders';
import { useCircuitBreakers, useResetCircuitBreaker } from '@/hooks/useHealth';
import { Spinner } from '@/components/ui/spinner';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { GeneralTab } from '@/components/GeneralTab';
import { CircuitBreakersTab } from '@/components/CircuitBreakersTab';
import {
  useEmbeddingConfig,
  useEmbeddingStatus,
  useUpdateEmbeddingConfig,
  useTestEmbeddingConfig,
  useEmbeddingModels,
} from '@/hooks/useEmbedding';
import { EMBEDDING_PROVIDERS, type EmbeddingProvider } from '@/lib/api/embedding';
import { ErrorBoundary } from '@/components/ErrorBoundary';

type Tab = 'models' | 'general' | 'circuit-breakers' | 'embeddings';

// ── Model Config Row ──────────────────────────────────────────────────────────

function ModelConfigRow({
  model,
  authStatus,
  contextWindow,
  maxTokens,
  contextStrategy,
  contextHighWaterMark,
  reasoning,
  onSaved,
}: {
  model: {
    modelName: string;
    provider?: string;
    api: string;
    baseUrl?: string;
    description?: string;
  };
  authStatus: string;
  contextWindow?: number;
  maxTokens?: number;
  contextStrategy?: string;
  contextHighWaterMark?: number;
  reasoning?: boolean;
  onSaved: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [localCtxWindow, setLocalCtxWindow] = useState(String(contextWindow ?? ''));
  const [localMaxTokens, setLocalMaxTokens] = useState(String(maxTokens ?? ''));
  const [localStrategy, setLocalStrategy] = useState(contextStrategy ?? 'sliding-window');
  const [localHighWater, setLocalHighWater] = useState(String(contextHighWaterMark ?? '0.8'));
  const [localReasoning, setLocalReasoning] = useState(reasoning ?? false);

  const handleSave = async () => {
    setSaving(true);
    try {
      const overrides: Record<string, unknown> = {};
      if (localCtxWindow) overrides.contextWindow = parseInt(localCtxWindow, 10);
      if (localMaxTokens) overrides.maxTokens = parseInt(localMaxTokens, 10);
      if (localStrategy) overrides.contextStrategy = localStrategy;
      if (localHighWater) overrides.contextHighWaterMark = parseFloat(localHighWater);
      overrides.reasoning = localReasoning;

      await updateProviderConfig(model.modelName, overrides);
      toast.success(`Config updated for ${model.modelName}`);
      onSaved();
    } catch (err) {
      toast.error(`Failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="border border-sera-border rounded-lg overflow-hidden">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center justify-between p-3 hover:bg-sera-surface/50 transition-colors text-left"
      >
        <div className="flex items-center gap-3">
          <span className="text-sera-text font-mono text-xs">{model.modelName}</span>
          <span className="text-sera-text-dim text-[10px]">{model.provider ?? ''}</span>
          <Badge
            variant={
              authStatus === 'configured'
                ? 'success'
                : authStatus === 'missing'
                  ? 'warning'
                  : 'default'
            }
            className="text-[9px]"
          >
            {authStatus === 'configured'
              ? 'Active'
              : authStatus === 'missing'
                ? 'Key missing'
                : 'Local'}
          </Badge>
          {reasoning && (
            <span className="flex items-center gap-0.5 text-[9px] text-violet-400">
              <Brain size={10} /> reasoning
            </span>
          )}
        </div>
        <ChevronDown
          size={14}
          className={`text-sera-text-dim transition-transform ${expanded ? 'rotate-180' : ''}`}
        />
      </button>

      {expanded && (
        <div className="border-t border-sera-border p-4 bg-sera-surface/30 space-y-3">
          <div className="grid grid-cols-2 lg:grid-cols-3 gap-3">
            <div>
              <label className="text-[10px] uppercase text-sera-text-dim block mb-1">
                Context Window
              </label>
              <input
                type="number"
                value={localCtxWindow}
                onChange={(e) => setLocalCtxWindow(e.target.value)}
                placeholder="128000"
                className="sera-input w-full text-xs"
              />
            </div>
            <div>
              <label className="text-[10px] uppercase text-sera-text-dim block mb-1">
                Max Tokens (output)
              </label>
              <input
                type="number"
                value={localMaxTokens}
                onChange={(e) => setLocalMaxTokens(e.target.value)}
                placeholder="4096"
                className="sera-input w-full text-xs"
              />
            </div>
            <div>
              <label className="text-[10px] uppercase text-sera-text-dim block mb-1">
                Context Strategy
              </label>
              <select
                value={localStrategy}
                onChange={(e) => setLocalStrategy(e.target.value)}
                className="sera-input w-full text-xs"
              >
                <option value="sliding-window">Sliding Window</option>
                <option value="summarize">Summarize</option>
                <option value="truncate">Truncate</option>
              </select>
            </div>
            <div>
              <label className="text-[10px] uppercase text-sera-text-dim block mb-1">
                High Water Mark
              </label>
              <input
                type="number"
                step="0.05"
                min="0.5"
                max="0.95"
                value={localHighWater}
                onChange={(e) => setLocalHighWater(e.target.value)}
                className="sera-input w-full text-xs"
              />
            </div>
            <div className="flex items-center gap-2 pt-4">
              <input
                type="checkbox"
                checked={localReasoning}
                onChange={(e) => setLocalReasoning(e.target.checked)}
                id={`reasoning-${model.modelName}`}
                className="accent-sera-accent"
              />
              <label htmlFor={`reasoning-${model.modelName}`} className="text-xs text-sera-text">
                Reasoning model
              </label>
            </div>
          </div>
          <div className="flex justify-end pt-1">
            <Button size="sm" onClick={handleSave} disabled={saving}>
              <Save size={12} className="mr-1" />
              {saving ? 'Saving...' : 'Save Config'}
            </Button>
          </div>
          <p className="text-[10px] text-sera-text-dim">
            Context window controls SERA's compaction threshold. The actual model context depends on
            your provider's settings (e.g., LM Studio model configuration).
          </p>
        </div>
      )}
    </div>
  );
}

function SettingsPageContent() {
  const [tab, setTab] = useState<Tab>('models');

  const {
    data: providersData,
    isLoading: isLoadingProviders,
    isError: isErrorProviders,
    refetch: refetchProviders,
  } = useProviders();
  // eslint-disable-next-line @typescript-eslint/no-unused-vars -- reserved for future LLM config UI
  const { data: _llmConfig } = useLLMConfig();
  const { data: circuitBreakers, refetch: refetchCB } = useCircuitBreakers();
  const resetCB = useResetCircuitBreaker();

  const registeredModels = providersData?.providers ?? [];

  const handleRefetch = () => {
    void refetchProviders();
  };

  const tabs: { id: Tab; label: string; icon: React.ReactNode }[] = [
    { id: 'models', label: 'Models', icon: <Settings2 size={14} /> },
    { id: 'circuit-breakers', label: 'Circuit Breakers', icon: <Activity size={14} /> },
    { id: 'embeddings', label: 'Embeddings', icon: <Search size={14} /> },
    { id: 'general', label: 'General', icon: <Sliders size={14} /> },
  ];

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Settings</h1>
          <p className="text-sm text-sera-text-muted mt-1">
            Configure models, embeddings, and system behavior
          </p>
        </div>
      </div>

      <div className="flex items-center gap-1 border-b border-sera-border mb-8">
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`flex items-center gap-2 px-4 py-3 text-sm font-medium border-b-2 transition-colors duration-150 ${
              tab === t.id
                ? 'border-sera-accent text-sera-accent'
                : 'border-transparent text-sera-text-muted hover:text-sera-text'
            }`}
          >
            {t.icon}
            {t.label}
          </button>
        ))}
      </div>

      {isLoadingProviders ? (
        <div className="flex items-center justify-center py-20">
          <Spinner />
        </div>
      ) : isErrorProviders ? (
        <div className="flex flex-col items-center justify-center p-8 border border-sera-border rounded-xl bg-sera-surface mt-4">
          <p className="text-sera-error mb-4">Failed to load model settings.</p>
          <Button onClick={handleRefetch} variant="outline">
            Retry
          </Button>
        </div>
      ) : (
        <>
          {tab === 'models' && (
            <div className="sera-card-static p-5">
              {registeredModels.length === 0 ? (
                <div className="text-center py-12 text-sera-text-dim text-sm">
                  No models registered. Add a provider in the Providers page.
                </div>
              ) : (
                <div className="space-y-2">
                  {registeredModels.map((m) => {
                    const ext = m as unknown as Record<string, unknown>;
                    return (
                      <ModelConfigRow
                        key={m.modelName}
                        model={m}
                        authStatus={ext.authStatus as string}
                        contextWindow={ext.contextWindow as number | undefined}
                        maxTokens={ext.maxTokens as number | undefined}
                        contextStrategy={ext.contextStrategy as string | undefined}
                        contextHighWaterMark={ext.contextHighWaterMark as number | undefined}
                        reasoning={ext.reasoning as boolean | undefined}
                        onSaved={() => void refetchProviders()}
                      />
                    );
                  })}
                </div>
              )}
            </div>
          )}

          {tab === 'circuit-breakers' && (
            <CircuitBreakersTab
              breakers={circuitBreakers ?? []}
              onReset={(p) => {
                void resetCB.mutateAsync(p).then(() => {
                  void refetchCB();
                });
              }}
              resetting={resetCB.isPending}
            />
          )}

          {tab === 'embeddings' && <EmbeddingsTab />}
          {tab === 'general' && <GeneralTab registeredModels={registeredModels} />}
        </>
      )}
    </div>
  );
}

export default function SettingsPage() {
  return (
    <ErrorBoundary fallbackMessage="The settings page encountered an error.">
      <SettingsPageContent />
    </ErrorBoundary>
  );
}

function EmbeddingsTab() {
  const {
    data: config,
    isLoading: configLoading,
    isError: configError,
    refetch: refetchConfig,
  } = useEmbeddingConfig();
  const { data: status } = useEmbeddingStatus();
  const updateConfig = useUpdateEmbeddingConfig();
  const testConfig = useTestEmbeddingConfig();

  const [provider, setProvider] = useState<EmbeddingProvider>('ollama');
  const [model, setModel] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [dimension, setDimension] = useState(768);
  const [initialized, setInitialized] = useState(false);
  const [testResult, setTestResult] = useState<{
    ok: boolean;
    latencyMs: number;
    dimension?: number;
    error?: string;
  } | null>(null);
  const [saveResult, setSaveResult] = useState<{
    dimensionChanged?: boolean;
    warning?: string;
  } | null>(null);

  const { data: modelsData } = useEmbeddingModels(provider, baseUrl);

  // Initialize form from loaded config
  useEffect(() => {
    if (config && !initialized) {
      setProvider(config.provider);
      setModel(config.model);
      setBaseUrl(config.baseUrl);
      setDimension(config.dimension);
      setInitialized(true);
    }
  }, [config, initialized]);

  const handleTest = async () => {
    setTestResult(null);
    const result = await testConfig.mutateAsync({
      provider,
      model,
      baseUrl,
      dimension,
      ...(apiKey ? { apiKey } : {}),
    });
    setTestResult(result);
  };

  const handleSave = async () => {
    setSaveResult(null);
    const result = await updateConfig.mutateAsync({
      provider,
      model,
      baseUrl,
      dimension,
      ...(apiKey ? { apiKey } : {}),
    });
    setSaveResult(result);
    setTestResult(result.testResult);
  };

  if (configLoading) return <Spinner />;

  if (configError) {
    return (
      <div className="flex flex-col items-center justify-center p-8 border border-sera-border rounded-xl bg-sera-surface">
        <p className="text-sera-error mb-4">Failed to load embedding settings.</p>
        <Button onClick={() => void refetchConfig()} variant="outline">
          Retry
        </Button>
      </div>
    );
  }

  const needsApiKey = provider === 'openai' || provider === 'openai-compatible';
  const needsBaseUrl = provider !== 'openai';

  return (
    <div className="space-y-6">
      {/* Status Card */}
      <div className="sera-card-static p-4">
        <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
          Embedding Status
        </h3>
        <div className="flex items-center gap-4 text-xs">
          <Badge variant={status?.available ? 'success' : 'default'}>
            {status?.available ? 'Active' : status?.configured ? 'Offline' : 'Not configured'}
          </Badge>
          {status?.model && (
            <span className="text-sera-text-muted">
              {status.provider}/{status.model} ({status.dimension}d)
            </span>
          )}
        </div>
      </div>

      {/* Configuration Form */}
      <div className="sera-card-static p-4 space-y-4">
        <h3 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider">
          Embedding Configuration
        </h3>

        {/* Provider */}
        <div>
          <label className="block text-xs text-sera-text-muted mb-1">Provider</label>
          <select
            value={provider}
            onChange={(e) => {
              const p = e.target.value as EmbeddingProvider;
              setProvider(p);
              // Auto-fill default baseUrl
              if (p === 'ollama') setBaseUrl('http://host.docker.internal:11434');
              else if (p === 'lm-studio') setBaseUrl('http://host.docker.internal:1234');
              else if (p === 'openai') setBaseUrl('https://api.openai.com');
              else setBaseUrl('');
            }}
            className="sera-input text-xs w-full"
          >
            {EMBEDDING_PROVIDERS.map((p) => (
              <option key={p.value} value={p.value}>
                {p.label}
              </option>
            ))}
          </select>
        </div>

        {/* Base URL */}
        {needsBaseUrl && (
          <div>
            <label className="block text-xs text-sera-text-muted mb-1">Base URL</label>
            <input
              type="text"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="http://host.docker.internal:11434"
              className="sera-input text-xs w-full font-mono"
            />
          </div>
        )}

        {/* API Key */}
        {needsApiKey && (
          <div>
            <label className="block text-xs text-sera-text-muted mb-1">API Key</label>
            <input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={config?.apiKey ? '(configured — enter to change)' : 'sk-...'}
              className="sera-input text-xs w-full font-mono"
            />
          </div>
        )}

        {/* Model */}
        <div>
          <label className="block text-xs text-sera-text-muted mb-1">Model</label>
          <input
            type="text"
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder="nomic-embed-text"
            className="sera-input text-xs w-full font-mono"
          />
          {/* Discovered models as chips */}
          {modelsData?.models && modelsData.models.length > 0 && (
            <div className="flex flex-wrap gap-1.5 mt-2">
              {modelsData.models.map((m) => (
                <button
                  key={m.id}
                  onClick={() => {
                    setModel(m.id);
                    if (m.dimension) setDimension(m.dimension);
                  }}
                  className={`px-2 py-0.5 rounded text-[11px] border transition-colors ${
                    model === m.id
                      ? 'bg-sera-accent-soft border-sera-accent text-sera-accent'
                      : 'border-sera-border text-sera-text-muted hover:text-sera-text hover:border-sera-text-muted'
                  }`}
                >
                  {m.id} {m.dimension ? `(${m.dimension}d)` : ''}
                </button>
              ))}
            </div>
          )}
        </div>

        {/* Dimension */}
        <div>
          <label className="block text-xs text-sera-text-muted mb-1">Vector Dimension</label>
          <input
            type="number"
            value={dimension}
            onChange={(e) => setDimension(parseInt(e.target.value, 10) || 768)}
            className="sera-input text-xs w-32 font-mono"
          />
          <p className="text-[10px] text-sera-text-dim mt-1">
            Auto-filled for known models. Changing dimension requires re-indexing.
          </p>
        </div>

        {/* Actions */}
        <div className="flex items-center gap-3 pt-2">
          <Button
            size="sm"
            variant="outline"
            onClick={() => void handleTest()}
            disabled={testConfig.isPending || !model || !baseUrl}
          >
            {testConfig.isPending ? <Spinner /> : <Zap size={13} />}
            Test Connection
          </Button>
          <Button
            size="sm"
            onClick={() => void handleSave()}
            disabled={updateConfig.isPending || !model || !baseUrl}
          >
            {updateConfig.isPending ? <Spinner /> : <Save size={13} />}
            Save
          </Button>
        </div>

        {/* Test Result */}
        {testResult && (
          <div
            className={`p-3 rounded-lg text-xs ${
              testResult.ok
                ? 'bg-sera-success/10 border border-sera-success/20 text-sera-success'
                : 'bg-sera-error/10 border border-sera-error/20 text-sera-error'
            }`}
          >
            {testResult.ok ? (
              <span className="flex items-center gap-2">
                <CheckCircle size={14} />
                Connected — {testResult.latencyMs}ms latency, {testResult.dimension}d vectors
              </span>
            ) : (
              <span className="flex items-center gap-2">
                <XCircle size={14} />
                Failed: {testResult.error}
              </span>
            )}
          </div>
        )}

        {/* Dimension Change Warning */}
        {saveResult?.dimensionChanged && (
          <div className="p-3 rounded-lg text-xs bg-yellow-500/10 border border-yellow-500/20 text-yellow-400 flex items-start gap-2">
            <AlertTriangle size={14} className="mt-0.5 flex-shrink-0" />
            <span>{saveResult.warning}</span>
          </div>
        )}
      </div>
    </div>
  );
}
