'use client';

import { useState, useEffect } from 'react';
import {
  Zap, Save, CheckCircle, XCircle, RefreshCw, ChevronDown, ChevronUp,
  Server, Cloud, Radio, Layers, Settings2, Sliders
} from 'lucide-react';

// ─── Types ────────────────────────────────────────────────────────────────────
interface ProviderModel {
  id: string;
  name: string;
  tier: 'frontier' | 'smart' | 'balanced' | 'fast' | 'local';
  contextWindow: number;
}

interface Provider {
  id: string;
  name: string;
  category: 'local' | 'cloud';
  defaultBaseUrl: string;
  requiresKey: boolean;
  description: string;
  models: ProviderModel[];
  configured: boolean;
  isActive: boolean;
  savedConfig: { baseUrl: string; apiKey: string; model: string } | null;
}

type Tab = 'providers' | 'models' | 'general';
type TestStatus = 'idle' | 'testing' | 'success' | 'error';

// ─── Tier Badge ───────────────────────────────────────────────────────────────
function TierBadge({ tier }: { tier: string }) {
  const colors: Record<string, string> = {
    frontier: 'bg-purple-500/15 text-purple-400',
    smart: 'bg-cyan-500/15 text-cyan-400',
    balanced: 'bg-blue-500/15 text-blue-400',
    fast: 'bg-emerald-500/15 text-emerald-400',
    local: 'bg-amber-500/15 text-amber-400',
  };
  return (
    <span className={`sera-badge ${colors[tier] || colors.fast}`}>
      {tier}
    </span>
  );
}

// ─── Provider Card ────────────────────────────────────────────────────────────
function ProviderCard({
  provider,
  onSave,
  onTest,
  onSetActive,
}: {
  provider: Provider;
  onSave: (id: string, config: { baseUrl: string; apiKey: string; model: string }) => Promise<void>;
  onTest: (id: string) => Promise<{ success: boolean; error?: string }>;
  onSetActive: (id: string) => Promise<void>;
}) {
  const [expanded, setExpanded] = useState(false);
  const [baseUrl, setBaseUrl] = useState(provider.savedConfig?.baseUrl || provider.defaultBaseUrl);
  const [apiKey, setApiKey] = useState(provider.savedConfig?.apiKey || '');
  const [model, setModel] = useState(provider.savedConfig?.model || provider.models[0]?.id || '');
  const [testStatus, setTestStatus] = useState<TestStatus>('idle');
  const [testMessage, setTestMessage] = useState('');
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  const handleTest = async () => {
    setTestStatus('testing');
    const result = await onTest(provider.id);
    setTestStatus(result.success ? 'success' : 'error');
    setTestMessage(result.success ? 'Connection successful' : (result.error || 'Connection failed'));
  };

  const handleSave = async () => {
    setSaving(true);
    await onSave(provider.id, { baseUrl, apiKey, model });
    setSaving(false);
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  const borderClass = provider.isActive
    ? 'border-sera-accent/40 shadow-[0_0_15px_rgba(0,229,255,0.08)]'
    : provider.configured
      ? 'border-sera-success/30'
      : '';

  return (
    <div className={`sera-card-static overflow-hidden ${borderClass}`}>
      {/* Card Header */}
      <button
        onClick={() => setExpanded(!expanded)}
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
              {provider.isActive && (
                <span className="sera-badge-accent">Active</span>
              )}
            </div>
            <p className="text-[11px] text-sera-text-muted mt-0.5">{provider.description}</p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <span className="text-[11px] text-sera-text-dim">
            {provider.models.length} model{provider.models.length !== 1 ? 's' : ''}
          </span>
          {provider.configured ? (
            <span className="w-2 h-2 rounded-full bg-sera-success" />
          ) : (
            <span className="w-2 h-2 rounded-full bg-sera-text-dim/30" />
          )}
          {expanded ? <ChevronUp size={14} className="text-sera-text-dim" /> : <ChevronDown size={14} className="text-sera-text-dim" />}
        </div>
      </button>

      {/* Expanded Config */}
      {expanded && (
        <div className="border-t border-sera-border p-4 space-y-4 bg-sera-bg/50">
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-1.5 col-span-2">
              <label className="text-[11px] font-medium text-sera-text-dim uppercase tracking-wider">Base API URL</label>
              <input
                type="text"
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                className="sera-input font-mono text-xs"
              />
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
              <select
                value={model}
                onChange={(e) => setModel(e.target.value)}
                className="sera-input text-xs appearance-none"
              >
                {provider.models.map((m) => (
                  <option key={m.id} value={m.id}>{m.name}</option>
                ))}
              </select>
            </div>
          </div>

          {/* Test result */}
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

          {saved && (
            <div className="flex items-center gap-2 px-3 py-2 rounded-lg text-xs border bg-sera-accent-soft border-sera-border-active text-sera-accent">
              <CheckCircle size={14} />
              <span>Configuration saved</span>
            </div>
          )}

          {/* Actions */}
          <div className="flex gap-3">
            <button
              onClick={handleTest}
              disabled={testStatus === 'testing'}
              className="sera-btn-ghost flex-1 border border-sera-border py-2.5 text-xs disabled:opacity-30"
            >
              {testStatus === 'testing' ? <RefreshCw className="animate-spin" size={14} /> : <Zap size={14} />}
              Test Connection
            </button>
            <button
              onClick={handleSave}
              disabled={saving}
              className="sera-btn-primary flex-1 text-xs disabled:opacity-30"
            >
              {saving ? <RefreshCw className="animate-spin" size={14} /> : <Save size={14} />}
              Save Config
            </button>
            {!provider.isActive && provider.configured && (
              <button
                onClick={() => onSetActive(provider.id)}
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

// ─── Main Settings Page ───────────────────────────────────────────────────────
export default function SettingsPage() {
  const [tab, setTab] = useState<Tab>('providers');
  const [providers, setProviders] = useState<Provider[]>([]);
  const [llmConfig, setLlmConfig] = useState<{ temperature?: number }>({});
  const [loading, setLoading] = useState(true);

  const fetchData = async () => {
    try {
      const [providersRes, llmRes] = await Promise.all([
        fetch('/api/core/providers'),
        fetch('/api/core/config/llm')
      ]);
      const data = await providersRes.json();
      setProviders(data.providers);

      const llmData = await llmRes.json();
      setLlmConfig(llmData);
    } catch (err) {
      console.error('Failed to fetch data:', err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { fetchData(); }, []);

  const handleSave = async (id: string, cfg: { baseUrl: string; apiKey: string; model: string }) => {
    await fetch(`/api/core/providers/${id}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(cfg),
    });
    await fetchData();
  };

  const handleTest = async (id: string) => {
    const res = await fetch(`/api/core/providers/${id}/test`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
    });
    return await res.json();
  };

  const handleSetActive = async (id: string) => {
    await fetch('/api/core/providers/active', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ providerId: id }),
    });
    await fetchData();
  };

  const localProviders = providers.filter(p => p.category === 'local');
  const cloudProviders = providers.filter(p => p.category === 'cloud');
  const allModels = providers.filter(p => p.configured).flatMap(p => p.models.map(m => ({ ...m, provider: p.name })));

  const tabs: { id: Tab; label: string; icon: React.ReactNode }[] = [
    { id: 'providers', label: 'Providers', icon: <Layers size={14} /> },
    { id: 'models', label: 'Models', icon: <Settings2 size={14} /> },
    { id: 'general', label: 'General', icon: <Sliders size={14} /> },
  ];

  return (
    <div className="p-8 max-w-5xl mx-auto">
      {/* Header */}
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Settings</h1>
          <p className="text-sm text-sera-text-muted mt-1">Configure providers, models, and system behavior</p>
        </div>
      </div>

      {/* Tabs */}
      <div className="flex items-center gap-1 border-b border-sera-border mb-8">
        {tabs.map(t => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`flex items-center gap-2 px-4 py-3 text-sm font-medium
              border-b-2 transition-colors duration-150
              ${tab === t.id
                ? 'border-sera-accent text-sera-accent'
                : 'border-transparent text-sera-text-muted hover:text-sera-text'
              }`}
          >
            {t.icon}
            {t.label}
          </button>
        ))}
      </div>

      {/* Content */}
      {loading ? (
        <div className="flex items-center justify-center py-20">
          <RefreshCw className="animate-spin text-sera-accent" size={24} />
        </div>
      ) : (
        <>
          {/* ─── Providers Tab ──────────────────────────────────────────── */}
          {tab === 'providers' && (
            <div className="space-y-8">
              {/* Local Providers */}
              <section>
                <div className="flex items-center gap-2 mb-4">
                  <Server size={14} className="text-amber-400" />
                  <h2 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase">
                    Local Providers
                  </h2>
                  <span className="text-[11px] text-sera-text-dim/60">
                    — Running on your homelab
                  </span>
                </div>
                <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                  {localProviders.map(p => (
                    <ProviderCard key={p.id} provider={p} onSave={handleSave} onTest={handleTest} onSetActive={handleSetActive} />
                  ))}
                </div>
              </section>

              {/* Cloud Providers */}
              <section>
                <div className="flex items-center gap-2 mb-4">
                  <Cloud size={14} className="text-sera-accent" />
                  <h2 className="text-xs font-semibold tracking-[0.1em] text-sera-text-dim uppercase">
                    Cloud Providers
                  </h2>
                  <span className="text-[11px] text-sera-text-dim/60">
                    — API key required
                  </span>
                </div>
                <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                  {cloudProviders.map(p => (
                    <ProviderCard key={p.id} provider={p} onSave={handleSave} onTest={handleTest} onSetActive={handleSetActive} />
                  ))}
                </div>
              </section>
            </div>
          )}

          {/* ─── Models Tab ─────────────────────────────────────────────── */}
          {tab === 'models' && (
            <div className="space-y-4">
              <div className="sera-card-static p-5 rounded-xl">
                <p className="text-xs text-sera-text-muted mb-4">
                  Models from configured providers. Configure a provider first to see its models.
                </p>
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
                              {m.contextWindow >= 1000000 ? `${(m.contextWindow / 1000000).toFixed(1)}M` : `${(m.contextWindow / 1000).toFixed(0)}K`}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
              </div>
            </div>
          )}

          {/* ─── General Tab ────────────────────────────────────────────── */}
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
                      <span className="text-xs text-sera-accent font-mono">{llmConfig?.temperature ?? 0.7}</span>
                    </div>
                    <input
                      type="range"
                      min="0"
                      max="1"
                      step="0.1"
                      value={llmConfig?.temperature ?? 0.7}
                      readOnly
                      className="w-full accent-sera-accent opacity-70 cursor-not-allowed"
                      title="Temperature is currently configured via backend settings"
                    />
                    <div className="flex justify-between text-[10px] text-sera-text-dim">
                      <span>Precise (0)</span>
                      <span>Creative (1)</span>
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
                    <span className="text-sera-text">Next.js 16 + Tailwind v4</span>
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
