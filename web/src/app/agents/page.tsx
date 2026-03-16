'use client';

import { Bot, Plus, Settings as SettingsIcon, Shield, RefreshCw } from 'lucide-react';
import { useState, useEffect } from 'react';
import Link from 'next/link';

interface Agent {
  name: string;
  displayName: string;
  role: string;
  tier: number;
  circle: string;
  icon: string;
}

const TIER_LABELS: Record<number, { label: string; class: string }> = {
  1: { label: 'Tier 1', class: 'sera-badge-muted' },
  2: { label: 'Tier 2', class: 'sera-badge-warning' },
  3: { label: 'Tier 3', class: 'sera-badge-error' },
};

export default function AgentsPage() {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [reloading, setReloading] = useState(false);

  const fetchAgents = async () => {
    try {
      const res = await fetch('/api/core/agents');
      if (!res.ok) throw new Error(`Failed to fetch agents: ${res.statusText}`);
      const data = await res.json();
      setAgents(data);
      setError(null);
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchAgents();
  }, []);

  const handleReload = async () => {
    setReloading(true);
    try {
      await fetch('/api/core/agents/reload', { method: 'POST' });
      await fetchAgents();
    } catch (err: any) {
      setError(err.message);
    } finally {
      setReloading(false);
    }
  };

  return (
    <div className="p-8 max-w-7xl mx-auto">
      {/* Header */}
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Agents</h1>
          <p className="text-sm text-sera-text-muted mt-1">Manage and monitor your autonomous agents</p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={handleReload}
            disabled={reloading}
            className="sera-btn-ghost"
            title="Reload agent manifests from disk"
          >
            <RefreshCw size={16} className={reloading ? 'animate-spin' : ''} />
            Reload
          </button>
        </div>
      </div>

      {/* Loading */}
      {loading && (
        <div className="flex items-center justify-center py-20">
          <div className="flex items-center gap-3 text-sera-text-muted">
            <RefreshCw size={18} className="animate-spin" />
            <span className="text-sm">Loading agents…</span>
          </div>
        </div>
      )}

      {/* Error */}
      {error && (
        <div className="sera-card-static p-4 mb-6 border-sera-error/30 bg-sera-error/5">
          <p className="text-sm text-sera-error">{error}</p>
        </div>
      )}

      {/* Agents Grid */}
      {!loading && agents.length > 0 && (
        <section>
          <h2 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-4">
            Registered Agents ({agents.length})
          </h2>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
            {agents.map((agent) => {
              const tierInfo = TIER_LABELS[agent.tier] || TIER_LABELS[1];
              return (
                <Link
                  key={agent.name}
                  href={`/agents/${agent.name}`}
                  className="sera-card p-4 group cursor-pointer"
                >
                  <div className="flex items-start justify-between mb-3">
                    <div className="w-10 h-10 rounded-lg bg-sera-accent-soft flex items-center justify-center text-lg">
                      {agent.icon}
                    </div>
                    <div className="flex items-center gap-1.5">
                      <span className={tierInfo.class}>
                        <Shield size={10} className="inline mr-0.5" />
                        {tierInfo.label}
                      </span>
                    </div>
                  </div>
                  <h3 className="text-sm font-semibold text-sera-text group-hover:text-sera-accent transition-colors">
                    {agent.displayName}
                  </h3>
                  <p className="text-xs text-sera-text-muted mt-1 line-clamp-2">{agent.role}</p>
                  <div className="mt-3 flex items-center gap-2">
                    <span className="sera-badge-accent">{agent.circle}</span>
                    <span className="text-[11px] text-sera-text-dim font-mono">{agent.name}</span>
                  </div>
                </Link>
              );
            })}
          </div>
        </section>
      )}

      {/* Empty State */}
      {!loading && agents.length === 0 && !error && (
        <div className="flex flex-col items-center justify-center py-20">
          <div className="w-14 h-14 rounded-xl bg-sera-surface flex items-center justify-center mb-4">
            <Bot size={28} className="text-sera-text-dim" />
          </div>
          <h3 className="text-sm font-semibold text-sera-text mb-1">No agents found</h3>
          <p className="text-xs text-sera-text-muted text-center max-w-sm">
            Create AGENT.yaml files in the <code className="text-sera-accent">sera/agents/</code> directory to register agents.
          </p>
        </div>
      )}
    </div>
  );
}
