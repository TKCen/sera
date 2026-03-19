import { useState } from 'react';
import {
  Zap, Save, CheckCircle, XCircle, RefreshCw, ChevronDown, ChevronUp,
  Server, Cloud, Radio, Layers, Settings2, Sliders
} from 'lucide-react';
import { useProviders, useUpdateProvider, useSetActiveProvider, useLLMConfig } from '@/hooks/useProviders';
import * as providersApi from '@/lib/api/providers';
import { Spinner } from '@/components/ui/spinner';

type Tab = 'providers' | 'models' | 'general';
type TestStatus = 'idle' | 'testing' | 'success' | 'error';

interface ProviderModel {
  id: string;
  name: string;
  tier: string;
  contextWindow: number;
}

interface ProviderExtended {
  id: string;
  name: string;
  category?: 'local' | 'cloud';
  defaultBaseUrl?: string;
  requiresKey?: boolean;
  description?: string;
  models?: ProviderModel[];
  configured?: boolean;
  isActive?: boolean;
  savedConfig?: { baseUrl: string; apiKey: string; model: string } | null;
}

function TierBadge({ tier }: { tier: string }) {
  const colors: Record<string, string> = {
    frontier: 'bg-purple-500/15 text-purple-400',
    smart: 'bg-cyan-500/15 text-cyan-400',
    balanced: 'bg-blue-500/15 text-blue-400',
    fast: 'bg-emerald-500/15 text-emerald-400',
    local: 'bg-amber-500/15 text-amber-400',
  };
  return (
    <span className={`sera-badge ${colors[tier] ?? 'bg-sera-surface-hover text-sera-text-muted'}`}>
      {tier}
    </span>
  );
}

function ProviderCard({ provider }: { provider: ProviderExtended }) {
  const [expanded, setExpanded] = useState(false);
  const [baseUrl, setBaseUrl] = useState(provider.savedConfig?.baseUrl ?? provider.defaultBaseUrl ?? '');
  const [apiKey, setApiKey] = useState(provider.savedConfig?.apiKey ?? '');
  const [model, setModel] = useState(provider.savedConfig?.model ?? provider.models?.[0]?.id ?? '');
  const [testStatus, setTestStatus] = useState<TestStatus>('idle');
  const [testMessage, setTestMessage] = useState('');

  const updateProvider = useUpdateProvider();
  const setActive = useSetActiveProvider();

  const handleTest = async () => {
    setTestStatus('testing');
    try {
      const result = await providersApi.testProvider(provider.id);
      setTestStatus(result.success ? 'success' : 'error');
      setTestMessage(result.success ? 'Connection successful' : (result.error ?? 'Connection failed'));
    } catch {
      setTestStatus('error');
      setTestMessage('Connection failed');
    }
  };

  const handleSave = () => {
    updateProvider.mutate({ id: provider.id, config: { baseUrl, apiKey, model } });
  };

  const borderClass = provider.isActive
    ? 'border-sera-accent/40 shadow-[0_0_15px_rgba(0,229,255,0.08)]'
    : provider.configured ? 'border-sera-success/30' : '';

  return (
    <div className={`sera-card-static overflow-hidden ${borderClass}`}>
      <button
        onClick={() => setExpanded((e) => !e)}
        className="w-full p-4 flex items-center justify-between hover:bg-sera-surface-hover transition-colors"
      >
        <div className="flex items-center gap-3">
          <div className={`w-9 h-9 rounded-lg flex items-center justify-center ${
            provider.category === 'local'
              ? 'bg-amber-500/10 border border-amber-500/20'
              : 'bg-sera-accent-soft border border-sera-border-active'
          }`}>
            {provider.category === 'local' ? (
              <Server size={16} className="text-amber-400" />
            ) : (
              <Cloud size={16} className="text-sera-accent" />
            )}
          </div>
          <div className="text-left">
            <div className="flex items-center gap-2">
              <h3 className="text-sm font-semibold text-sera-text">{provider.name}</h3>
              {provider.isActive && <span className="sera-badge-accent">Active</span>}
            </div>
            <p className="text-[11px] text-sera-text-muted mt-0.5">{provider.description}</p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <span className="text-[11px] text-sera-text-dim">
            {provider.models?.length ?? 0} model{provider.models?.length !== 1 ? 's' : ''}
          </span>
          {provider.configured ? (
            <span className="w-2 h-2 rounded-full bg-sera-success" />
          ) : (
            <span className="w-2 h-2 rounded-full bg-sera-text-dim/30" />
          )}
          {expanded ? <ChevronUp size={14} className="text-sera-text-dim" /> : <ChevronDown size={14} className="text-sera-text-dim" />}
        </div>
      </button>

      {expanded && (
        <div className="border-t border-sera-border p-4 space-y-4 bg-sera-bg/50">
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-1.5 col-span-2">
              <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">Base API URL</label>
              <input type="text" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} className="sera-input font-mono text-xs" />
            </div>
            <div className="space-y-1.5">
              <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">
                API Key {!provider.requiresKey && <span className="text-sera-text-dim/50">(optional)</span>}
              </label>
              <input
                type="password"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder={provider.requiresKey ? 'Required' : 'Not required'}
                className="sera-input text-xs"
              />
            </div>
            <div className="space-y-1.5">
              <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">Model</label>
              <select value={model} onChange={(e) => setModel(e.target.value)} className="sera-input text-xs appearance-none">
                {provider.models?.map((m) => (
                  <option key={m.id} value={m.id}>{m.name}</option>
                ))}
              </select>
            </div>
          </div>

          {testStatus !== 'idle' && testStatus !== 'testing' && (
            <div className={`flex items-center gap-2 px-3 py-2 rounded-lg text-xs border ${
              testStatus === 'success'
                ? 'bg-sera-success/10 border-sera-success/30 text-sera-success'
                : 'bg-sera-error/10 border-sera-error/30 text-sera-error'
            }`}>
              {testStatus === 'success' ? <CheckCircle size={14} /> : <XCircle size={14} />}
              <span>{testMessage}</span>
            </div>
          )}

          {updateProvider.isSuccess && (
            <div className="flex items-center gap-2 px-3 py-2 rounded-lg text-xs border bg-sera-accent-soft border-sera-border-active text-sera-accent">
              <CheckCircle size={14} />
              <span>Configuration saved</span>
            </div>
          )}

          <div className="flex gap-3">
            <button
              onClick={() => { void handleTest(); }}
              disabled={testStatus === 'testing'}
              className="sera-btn-ghost flex-1 border border-sera-border py-2.5 text-xs disabled:opacity-30"
            >
              {testStatus === 'testing' ? <RefreshCw className="animate-spin" size={14} /> : <Zap size={14} />}
              Test Connection
            </button>
            <button
              onClick={handleSave}
              disabled={updateProvider.isPending}
              className="sera-btn-primary flex-1 text-xs disabled:opacity-30"
            >
              {updateProvider.isPending ? <RefreshCw className="animate-spin" size={14} /> : <Save size={14} />}
              Save Config
            </button>
            {!provider.isActive && provider.configured && (
              <button
                onClick={() => setActive.mutate(provider.id)}
                className="inline-flex items-center gap-2 bg-sera-success/10 hover:bg-sera-success/20 text-sera-success border border-sera-success/30 py-2.5 px-4 rounded-lg text-xs transition-all"
              >
                <Radio size={14} />
                Set Active
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

export default function SettingsPage() {
  const [tab, setTab] = useState<Tab>('providers');
  const { data: providersData, isLoading } = useProviders();
  const { data: llmConfig } = useLLMConfig();

  const providers = (providersData?.providers ?? []) as ProviderExtended[];
  const localProviders = providers.filter((p) => p.category === 'local');
  const cloudProviders = providers.filter((p) => p.category === 'cloud');
  const allModels = providers
    .filter((p) => p.configured)
    .flatMap((p) => (p.models ?? []).map((m) => ({ ...m, provider: p.name })));

  const tabs: { id: Tab; label: string; icon: React.ReactNode }[] = [
    { id: 'providers', label: 'Providers', icon: <Layers size={14} /> },
    { id: 'models', label: 'Models', icon: <Settings2 size={14} /> },
    { id: 'general', label: 'General', icon: <Sliders size={14} /> },
  ];

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Settings</h1>
          <p className="text-sm text-sera-text-muted mt-1">Configure providers, models, and system behavior</p>
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
            <div className="space-y-8">
              <section>
                <div className="flex items-center gap-2 mb-4">
                  <Server size={14} className="text-amber-400" />
                  <h2 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase">Local Providers</h2>
                  <span className="text-[11px] text-sera-text-dim/60">— Running on your homelab</span>
                </div>
                <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                  {localProviders.map((p) => <ProviderCard key={p.id} provider={p} />)}
                </div>
              </section>
              <section>
                <div className="flex items-center gap-2 mb-4">
                  <Cloud size={14} className="text-sera-accent" />
                  <h2 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase">Cloud Providers</h2>
                  <span className="text-[11px] text-sera-text-dim/60">— API key required</span>
                </div>
                <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                  {cloudProviders.map((p) => <ProviderCard key={p.id} provider={p} />)}
                </div>
              </section>
            </div>
          )}

          {tab === 'models' && (
            <div className="sera-card-static p-5">
              {allModels.length === 0 ? (
                <div className="text-center py-12 text-sera-text-dim text-sm">
                  No providers configured. Go to Providers tab to set up an LLM provider.
                </div>
              ) : (
                <div className="overflow-x-auto">
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="border-b border-sera-border text-[11px] uppercase tracking-wider text-sera-text-dim">
                        <th className="text-left py-3 px-3">Model</th>
                        <th className="text-left py-3 px-3">Provider</th>
                        <th className="text-left py-3 px-3">Tier</th>
                        <th className="text-right py-3 px-3">Context</th>
                      </tr>
                    </thead>
                    <tbody>
                      {allModels.map((m, i) => (
                        <tr key={`${m.provider}-${m.id}-${i}`} className="border-b border-sera-border/50 hover:bg-sera-surface-hover transition-colors">
                          <td className="py-3 px-3">
                            <span className="text-sera-text">{m.name}</span>
                            <span className="text-sera-text-dim text-[10px] block font-mono">{m.id}</span>
                          </td>
                          <td className="py-3 px-3 text-sera-text-muted">{m.provider}</td>
                          <td className="py-3 px-3"><TierBadge tier={m.tier} /></td>
                          <td className="py-3 px-3 text-right text-sera-text-muted font-mono text-xs">
                            {m.contextWindow >= 1000000
                              ? `${(m.contextWindow / 1000000).toFixed(1)}M`
                              : `${(m.contextWindow / 1000).toFixed(0)}K`}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </div>
          )}

          {tab === 'general' && (
            <div className="sera-card-static p-6 space-y-6 max-w-xl">
              <div>
                <h3 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase mb-4">Agent Defaults</h3>
                <div className="space-y-4">
                  <div className="space-y-1.5">
                    <div className="flex justify-between items-center mb-1">
                      <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">Temperature</label>
                      <span className="text-xs text-sera-accent font-mono">{llmConfig?.model ?? '—'}</span>
                    </div>
                  </div>
                </div>
              </div>
              <div className="border-t border-sera-border pt-6">
                <h3 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase mb-3">System Info</h3>
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
