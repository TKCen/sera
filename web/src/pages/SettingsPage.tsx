import { useState } from 'react';
import {
  Zap,
  CheckCircle,
  XCircle,
  RefreshCw,
  ChevronDown,
  ChevronUp,
  Radio,
  Layers,
  Settings2,
  Sliders,
  Activity,
  Plus,
  Trash2,
  ExternalLink,
  Save,
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

type Tab = 'providers' | 'models' | 'general' | 'circuit-breakers';

function DynamicProviderCard({
  provider,
  status,
  onRemove,
}: {
  provider: providersApi.DynamicProviderConfig;
  status?: providersApi.DynamicProviderStatus;
  onRemove: (id: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const isHealthy = status?.status === 'ok';

  return (
    <div
      className={`sera-card-static overflow-hidden ${
        isHealthy
          ? 'border-sera-success/30'
          : 'border-sera-error/30 shadow-[0_0_15px_rgba(255,82,82,0.05)]'
      }`}
    >
      <button
        onClick={() => setExpanded((e) => !e)}
        className="w-full p-4 flex items-center justify-between hover:bg-sera-surface-hover transition-colors"
      >
        <div className="flex items-center gap-3">
          <div className="w-9 h-9 rounded-lg flex items-center justify-center bg-amber-500/10 border border-amber-500/20">
            <Radio size={16} className="text-amber-400" />
          </div>
          <div className="text-left">
            <div className="flex items-center gap-2">
              <h3 className="text-sm font-semibold text-sera-text">{provider.name}</h3>
              {isHealthy ? (
                <Badge variant="success" className="text-[9px] px-1.5 py-0">
                  Online
                </Badge>
              ) : (
                <Badge variant="error" className="text-[9px] px-1.5 py-0">
                  Offline
                </Badge>
              )}
            </div>
            <p className="text-[11px] text-sera-text-muted mt-0.5 font-mono select-all">
              {provider.baseUrl}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <span className="text-[11px] text-sera-text-dim px-2 py-0.5 rounded-full bg-sera-bg/80 border border-sera-border">
            {status?.discoveredModels?.length ?? 0} models
          </span>
          {expanded ? (
            <ChevronUp size={14} className="text-sera-text-dim" />
          ) : (
            <ChevronDown size={14} className="text-sera-text-dim" />
          )}
        </div>
      </button>

      {expanded && (
        <div className="border-t border-sera-border p-4 space-y-4 bg-sera-bg/50 animate-in slide-in-from-top-2 duration-200">
          <div className="space-y-3">
            <div className="flex justify-between items-center px-1">
              <span className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider flex items-center gap-1.5">
                <Activity size={10} /> Discovery Status
              </span>
              <span className="text-[11px] text-sera-text-muted font-mono bg-sera-surface/80 px-2 py-0.5 rounded border border-sera-border">
                {status?.lastCheck ? new Date(status.lastCheck).toLocaleTimeString() : 'Never'}
              </span>
            </div>

            {!isHealthy && status?.error && (
              <div className="flex items-start gap-2 p-3 rounded-lg bg-sera-error/5 border border-sera-error/20 text-sera-error text-[11px] leading-relaxed">
                <XCircle size={14} className="mt-0.5 shrink-0" />
                <span>{status.error}</span>
              </div>
            )}

            {isHealthy && (
              <div className="space-y-2">
                <span className="text-[11px] text-sera-text-dim px-1 block">Live Models:</span>
                <div className="flex flex-wrap gap-2 p-2 rounded-lg bg-sera-bg/30 border border-sera-border/50">
                  {status?.discoveredModels?.map((m) => (
                    <span
                      key={m}
                      className="px-2 py-1 rounded border border-sera-border bg-sera-surface/50 text-[10px] text-sera-text-muted font-mono hover:border-sera-accent/30 hover:text-sera-text transition-colors cursor-default"
                    >
                      {m}
                    </span>
                  ))}
                  {status?.discoveredModels?.length === 0 && (
                    <span className="text-[11px] text-sera-text-dim italic px-2 py-1">
                      No models found — start them in LM Studio
                    </span>
                  )}
                </div>
              </div>
            )}
          </div>

          <div className="flex gap-2 border-t border-sera-border pt-4 mt-2">
            <Button
              variant="outline"
              size="sm"
              className="flex-1 text-xs h-9 bg-sera-error/5 hover:bg-sera-error/10 text-sera-error border-sera-error/20 gap-2"
              onClick={() => onRemove(provider.id)}
            >
              <Trash2 size={13} /> Remove
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="flex-1 text-xs h-9 gap-2"
              onClick={() => window.open(provider.baseUrl, '_blank')}
            >
              <ExternalLink size={13} /> API Info
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

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
  const { data: llmConfig } = useLLMConfig();
  const { data: circuitBreakers, refetch: refetchCB } = useCircuitBreakers();
  const resetCB = useResetCircuitBreaker();

  const registeredModels = providersData?.providers ?? [];

  const isLoading = isLoadingProviders || isLoadingDynamic;

  const tabs: { id: Tab; label: string; icon: React.ReactNode }[] = [
    { id: 'providers', label: 'Providers', icon: <Layers size={14} /> },
    { id: 'models', label: 'Models', icon: <Settings2 size={14} /> },
    { id: 'circuit-breakers', label: 'Circuit Breakers', icon: <Activity size={14} /> },
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

          {tab === 'general' && (
            <div className="sera-card-static p-6 space-y-6 max-w-xl">
              <div>
                <h3 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase mb-4">
                  Agent Defaults
                </h3>
                <div className="space-y-4">
                  <div className="space-y-1.5">
                    <div className="flex justify-between items-center mb-1">
                      <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                        Temperature
                      </label>
                      <span className="text-xs text-sera-accent font-mono">
                        {llmConfig?.model ?? '—'}
                      </span>
                    </div>
                  </div>
                </div>
              </div>
              <div className="border-t border-sera-border pt-6">
                <h3 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase mb-3">
                  System Info
                </h3>
                <div className="space-y-2 text-xs">
                  <div className="flex justify-between">
                    <span className="text-sera-text-muted">Platform</span>
                    <span className="text-sera-text">SERA v1.0</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-sera-text-muted">Runtime</span>
                    <span className="text-sera-text">Node.js 20 + TypeScript</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-sera-text-muted">Frontend</span>
                    <span className="text-sera-text">Vite + React Router v7</span>
                  </div>
                </div>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}

function cbStateBadge(state: string) {
  if (state === 'open') return <Badge variant="error">Open</Badge>;
  if (state === 'half-open') return <Badge variant="warning">Half-Open</Badge>;
  return <Badge variant="success">Closed</Badge>;
}

function CircuitBreakersTab({
  breakers,
  onReset,
  resetting,
}: {
  breakers: Array<{
    provider: string;
    state: string;
    failures: number;
    lastFailureAt?: string;
    nextRetryAt?: string;
  }>;
  onReset: (provider: string) => void;
  resetting: boolean;
}) {
  return (
    <div className="space-y-4">
      <p className="text-sm text-sera-text-muted">
        Circuit breakers protect against repeated failures. When open, requests to the provider are
        paused.
      </p>
      {breakers.length === 0 ? (
        <div className="sera-card-static p-8 text-center text-sera-text-dim text-sm">
          No circuit breaker data — all providers healthy.
        </div>
      ) : (
        <div className="sera-card-static overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-sera-border text-[11px] uppercase tracking-wider text-sera-text-dim">
                <th className="text-left py-3 px-4">Provider</th>
                <th className="text-left py-3 px-4">State</th>
                <th className="text-left py-3 px-4">Failures</th>
                <th className="text-left py-3 px-4">Last Failure</th>
                <th className="text-left py-3 px-4">Next Retry</th>
                <th className="py-3 px-4" />
              </tr>
            </thead>
            <tbody>
              {breakers.map((cb) => (
                <tr
                  key={cb.provider}
                  className="border-b border-sera-border/50 hover:bg-sera-surface-hover transition-colors"
                >
                  <td className="py-3 px-4 font-mono text-xs text-sera-text">{cb.provider}</td>
                  <td className="py-3 px-4">{cbStateBadge(cb.state)}</td>
                  <td className="py-3 px-4 text-sera-text-muted">{cb.failures}</td>
                  <td className="py-3 px-4 text-xs text-sera-text-muted">
                    {cb.lastFailureAt ? new Date(cb.lastFailureAt).toLocaleString() : '—'}
                  </td>
                  <td className="py-3 px-4 text-xs text-sera-text-muted">
                    {cb.nextRetryAt ? new Date(cb.nextRetryAt).toLocaleString() : '—'}
                  </td>
                  <td className="py-3 px-4 text-right">
                    {cb.state !== 'closed' && (
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={resetting}
                        onClick={() => onReset(cb.provider)}
                      >
                        <RefreshCw size={12} /> Reset
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
