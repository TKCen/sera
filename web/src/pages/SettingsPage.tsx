import { useState, useEffect } from 'react';
import {
  Zap,
  CheckCircle,
  XCircle,
  RefreshCw,
  Radio,
  Layers,
  Settings2,
  Sliders,
  Activity,
  Plus,
  Save,
  Search,
  AlertTriangle,
} from 'lucide-react';
import {
  useProviders,
  useLLMConfig,
  useDynamicProviders,
  useDynamicProviderStatuses,
  useAddDynamicProvider,
  useRemoveDynamicProvider,
} from '@/hooks/useProviders';
import { useCircuitBreakers, useResetCircuitBreaker } from '@/hooks/useHealth';
import * as providersApi from '@/lib/api/providers';
import { Spinner } from '@/components/ui/spinner';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { CloudProviderSection } from '@/components/CloudProviderSection';
import { DynamicProviderCard } from '@/components/DynamicProviderCard';
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

type Tab = 'providers' | 'models' | 'general' | 'circuit-breakers' | 'embeddings';

export default function SettingsPage() {
  const [tab, setTab] = useState<Tab>('providers');
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

  const { data: providersData, isLoading: isLoadingProviders } = useProviders();
  const { data: dynamicData, isLoading: isLoadingDynamic } = useDynamicProviders();
  const { data: statusesData } = useDynamicProviderStatuses();
  const addDynamic = useAddDynamicProvider();
  const removeDynamic = useRemoveDynamicProvider();
  // eslint-disable-next-line @typescript-eslint/no-unused-vars -- reserved for future LLM config UI
  const { data: _llmConfig } = useLLMConfig();
  const { data: circuitBreakers, refetch: refetchCB } = useCircuitBreakers();
  const resetCB = useResetCircuitBreaker();

  const registeredModels = providersData?.providers ?? [];

  const isLoading = isLoadingProviders || isLoadingDynamic;

  const tabs: { id: Tab; label: string; icon: React.ReactNode }[] = [
    { id: 'providers', label: 'Providers', icon: <Layers size={14} /> },
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
            Configure providers, models, and system behavior
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

      {isLoading ? (
        <div className="flex items-center justify-center py-20">
          <Spinner />
        </div>
      ) : (
        <>
          {tab === 'providers' && (
            <div className="space-y-8 animate-in fade-in slide-in-from-bottom-2 duration-300">
              {/* Dynamic Providers Section */}
              <section>
                <div className="flex items-center justify-between mb-4">
                  <div className="flex items-center gap-2">
                    <Radio size={14} className="text-amber-400" />
                    <h2 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase">
                      Dynamic Discovery
                    </h2>
                    <span className="text-[11px] text-sera-text-dim/60">
                      — LM Studio, Ollama, etc.
                    </span>
                  </div>
                  <Button
                    size="sm"
                    className="h-8 text-[11px] gap-1.5 bg-sera-accent/10 hover:bg-sera-accent/20 text-sera-accent border border-sera-accent/20"
                    onClick={() => setShowAddDynamic(true)}
                  >
                    <Plus size={14} /> Add Provider
                  </Button>
                </div>

                {showAddDynamic && (
                  <div className="sera-card-static p-5 mb-4 border-sera-accent/30 bg-sera-accent/5 animate-in zoom-in-95 duration-200">
                    <div className="flex justify-between items-start mb-4">
                      <h3 className="text-sm font-semibold text-sera-text">
                        Add LM Studio Instance
                      </h3>
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
                        <input
                          type="text"
                          placeholder="e.g. Local LM Studio"
                          value={newDynamic.name}
                          onChange={(e) => setNewDynamic({ ...newDynamic, name: e.target.value })}
                          className="sera-input text-xs"
                        />
                      </div>
                      <div className="space-y-1.5">
                        <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                          Unique ID
                        </label>
                        <input
                          type="text"
                          placeholder="e.g. lmstudio-1"
                          value={newDynamic.id}
                          onChange={(e) =>
                            setNewDynamic({
                              ...newDynamic,
                              id: e.target.value.toLowerCase().replace(/\s+/g, '-'),
                            })
                          }
                          className="sera-input text-xs font-mono"
                        />
                      </div>
                      <div className="space-y-1.5 col-span-2">
                        <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                          Base URL (with /v1)
                        </label>
                        <input
                          type="text"
                          value={newDynamic.baseUrl}
                          onChange={(e) =>
                            setNewDynamic({ ...newDynamic, baseUrl: e.target.value })
                          }
                          className="sera-input text-xs font-mono"
                        />
                        <p className="text-[10px] text-sera-text-dim mt-0.5">
                          Running in Docker? Use{' '}
                          <code className="font-mono">host.docker.internal</code> instead of{' '}
                          <code className="font-mono">localhost</code>
                        </p>
                      </div>
                      <div className="space-y-1.5 col-span-2">
                        <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                          API Key <span className="text-sera-text-dim/50">(optional)</span>
                        </label>
                        <input
                          type="password"
                          value={newDynamic.apiKey}
                          onChange={(e) => setNewDynamic({ ...newDynamic, apiKey: e.target.value })}
                          className="sera-input text-xs"
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
                          {testResult.success ? <CheckCircle size={14} /> : <XCircle size={14} />}
                          <div>
                            <p className="font-semibold">
                              {testResult.success ? 'Connection successful' : 'Connection failed'}
                            </p>
                            {!testResult.success && (
                              <p className="mt-0.5 opacity-90">{testResult.error}</p>
                            )}
                            {testResult.success && (
                              <p className="mt-1 opacity-90">
                                Found {testResult.models.length} model(s):{' '}
                                {testResult.models.join(', ')}
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
                        {isTesting ? (
                          <RefreshCw className="animate-spin" size={14} />
                        ) : (
                          <Zap size={14} />
                        )}
                        Test & Discover
                      </Button>
                      <Button
                        className="flex-1 text-xs bg-sera-accent hover:bg-sera-accent-hover text-sera-bg h-10"
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
                      <div className="col-span-full sera-card-static border-dashed border-sera-border p-10 text-center">
                        <div className="w-12 h-12 rounded-full bg-sera-surface-hover flex items-center justify-center mx-auto mb-4">
                          <Radio size={20} className="text-sera-text-dim" />
                        </div>
                        <h3 className="text-sm font-medium text-sera-text mb-1">
                          No dynamic providers
                        </h3>
                        <p className="text-xs text-sera-text-muted mb-4">
                          Add an LM Studio or Ollama instance to discover models automatically.
                        </p>
                        <Button
                          size="sm"
                          variant="outline"
                          className="text-xs gap-1.5"
                          onClick={() => setShowAddDynamic(true)}
                        >
                          <Plus size={14} /> Configure Now
                        </Button>
                      </div>
                    )}
                </div>
              </section>

              {/* Cloud Providers Section */}
              <CloudProviderSection />
            </div>
          )}

          {tab === 'models' && (
            <div className="sera-card-static p-5">
              {registeredModels.length === 0 ? (
                <div className="text-center py-12 text-sera-text-dim text-sm">
                  No models registered. Add a provider in the Providers tab.
                </div>
              ) : (
                <div className="overflow-x-auto">
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="border-b border-sera-border text-[11px] uppercase tracking-wider text-sera-text-dim">
                        <th className="text-left py-3 px-3">Model name</th>
                        <th className="text-left py-3 px-3">Provider</th>
                        <th className="text-left py-3 px-3">API</th>
                        <th className="text-left py-3 px-3">Status</th>
                        <th className="text-left py-3 px-3">Base URL</th>
                      </tr>
                    </thead>
                    <tbody>
                      {registeredModels.map((m) => (
                        <tr
                          key={m.modelName}
                          className="border-b border-sera-border/50 hover:bg-sera-surface-hover transition-colors"
                        >
                          <td className="py-3 px-3">
                            <span className="text-sera-text font-mono text-xs">{m.modelName}</span>
                            {m.description && (
                              <span className="text-sera-text-dim text-[10px] block">
                                {m.description}
                              </span>
                            )}
                          </td>
                          <td className="py-3 px-3 text-sera-text-muted text-xs">
                            {m.provider ?? '—'}
                          </td>
                          <td className="py-3 px-3 text-sera-text-muted font-mono text-xs">
                            {m.api}
                          </td>
                          <td className="py-3 px-3">
                            {(m as unknown as Record<string, unknown>).authStatus ===
                              'configured' && (
                              <Badge variant="success" className="text-[9px]">
                                Active
                              </Badge>
                            )}
                            {(m as unknown as Record<string, unknown>).authStatus === 'missing' && (
                              <Badge variant="warning" className="text-[9px]">
                                Key missing
                              </Badge>
                            )}
                            {(m as unknown as Record<string, unknown>).authStatus ===
                              'not-required' && (
                              <Badge variant="default" className="text-[9px]">
                                Local
                              </Badge>
                            )}
                          </td>
                          <td className="py-3 px-3 text-sera-text-dim font-mono text-[10px]">
                            {m.baseUrl ?? '—'}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
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

function EmbeddingsTab() {
  const { data: config, isLoading: configLoading } = useEmbeddingConfig();
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
