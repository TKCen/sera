'use client';

import { useState, useEffect } from 'react';
import { Users, Bot, FileText, Radio, Plus, ArrowLeft, RefreshCw } from 'lucide-react';
import Link from 'next/link';

interface CircleSummary {
  name: string;
  displayName: string;
  description?: string;
  agents: string[];
  hasProjectContext: boolean;
  channelCount: number;
}

export default function CirclesPage() {
  const [circles, setCircles] = useState<CircleSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetch('/api/core/circles')
      .then(async (res) => {
        if (!res.ok) throw new Error('Failed to fetch circles');
        return res.json();
      })
      .then(setCircles)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, []);

  return (
    <div className="p-8 max-w-7xl mx-auto">
      {/* Header */}
      <div className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Circles</h1>
          <p className="text-sm text-sera-text-muted mt-1">Organize agents into collaborative teams</p>
        </div>
      </div>

      {/* Loading */}
      {loading && (
        <div className="flex items-center justify-center py-20">
          <div className="flex items-center gap-3 text-sera-text-muted">
            <RefreshCw size={18} className="animate-spin" />
            <span className="text-sm">Loading circles…</span>
          </div>
        </div>
      )}

      {/* Error */}
      {error && (
        <div className="sera-card-static p-4 mb-6 border-sera-error/30 bg-sera-error/5">
          <p className="text-sm text-sera-error">{error}</p>
        </div>
      )}

      {/* Circles Grid */}
      {!loading && circles.length > 0 && (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {circles.map((circle) => (
            <Link
              key={circle.name}
              href={`/circles/${circle.name}`}
              className="sera-card p-5 group cursor-pointer"
            >
              <div className="flex items-start justify-between mb-3">
                <div className="w-10 h-10 rounded-lg bg-purple-500/10 flex items-center justify-center">
                  <Users size={20} className="text-purple-400" />
                </div>
                <div className="flex items-center gap-1.5">
                  {circle.hasProjectContext && (
                    <span className="sera-badge bg-emerald-500/15 text-emerald-400">
                      <FileText size={10} className="inline mr-0.5" />
                      Context
                    </span>
                  )}
                </div>
              </div>

              <h3 className="text-sm font-semibold text-sera-text group-hover:text-sera-accent transition-colors">
                {circle.displayName}
              </h3>
              {circle.description && (
                <p className="text-xs text-sera-text-muted mt-1 line-clamp-2">{circle.description}</p>
              )}

              <div className="mt-3 flex items-center gap-3">
                <span className="text-[11px] text-sera-text-dim flex items-center gap-1">
                  <Bot size={11} />
                  {circle.agents.length} agent{circle.agents.length !== 1 ? 's' : ''}
                </span>
                {circle.channelCount > 0 && (
                  <span className="text-[11px] text-sera-text-dim flex items-center gap-1">
                    <Radio size={11} />
                    {circle.channelCount} channel{circle.channelCount !== 1 ? 's' : ''}
                  </span>
                )}
              </div>

              {/* Agent chips */}
              <div className="mt-3 flex flex-wrap gap-1">
                {circle.agents.slice(0, 5).map((agent) => (
                  <span key={agent} className="text-[10px] px-1.5 py-0.5 rounded bg-sera-surface-hover text-sera-text-muted">
                    {agent}
                  </span>
                ))}
                {circle.agents.length > 5 && (
                  <span className="text-[10px] px-1.5 py-0.5 rounded bg-sera-surface-hover text-sera-text-dim">
                    +{circle.agents.length - 5} more
                  </span>
                )}
              </div>
            </Link>
          ))}
        </div>
      )}

      {/* Empty State */}
      {!loading && circles.length === 0 && !error && (
        <div className="flex flex-col items-center justify-center py-20">
          <div className="w-14 h-14 rounded-xl bg-sera-surface flex items-center justify-center mb-4">
            <Users size={28} className="text-sera-text-dim" />
          </div>
          <h3 className="text-sm font-semibold text-sera-text mb-1">No circles found</h3>
          <p className="text-xs text-sera-text-muted text-center max-w-sm">
            Create CIRCLE.yaml files in the <code className="text-sera-accent">sera/circles/</code> directory to define circles.
          </p>
        </div>
      )}
    </div>
  );
}
