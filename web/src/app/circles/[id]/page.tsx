'use client';

import { useParams } from 'next/navigation';
import { useState, useEffect } from 'react';
import { Users, Bot, ArrowLeft, Radio, FileText, Gamepad2, Settings, Shield } from 'lucide-react';
import Link from 'next/link';

interface CircleDetail {
  apiVersion: string;
  kind: string;
  metadata: {
    name: string;
    displayName: string;
    description?: string;
  };
  agents: string[];
  projectContext?: { path: string } | null;
  knowledge?: { qdrantCollection: string };
  channels?: Array<{ name: string; type: string }>;
  partyMode?: { enabled: boolean; orchestrator?: string; selectionStrategy?: string };
  connections?: Array<{ circle: string }>;
  // injected by the API
  projectContextContent?: string | null;
}

export default function CircleDetailPage() {
  const params = useParams();
  const circleName = params.id as string;
  const [circle, setCircle] = useState<CircleDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetch(`/api/core/circles/${circleName}`)
      .then(async (res) => {
        if (!res.ok) throw new Error('Circle not found');
        const data = await res.json();
        // The API returns `projectContext` as the content string (or null)
        setCircle({ ...data, projectContextContent: data.projectContext });
        return;
      })
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [circleName]);

  if (loading) {
    return <div className="flex items-center justify-center h-full"><span className="text-sm text-sera-text-muted">Loading…</span></div>;
  }

  if (error || !circle) {
    return (
      <div className="p-8 max-w-5xl mx-auto">
        <Link href="/circles" className="inline-flex items-center gap-1.5 text-xs text-sera-text-dim hover:text-sera-text transition-colors mb-4">
          <ArrowLeft size={14} /> Back to Circles
        </Link>
        <div className="sera-card-static p-6 text-center">
          <Users size={32} className="text-sera-text-dim mx-auto mb-3" />
          <p className="text-sm text-sera-error">{error || 'Circle not found'}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="p-8 max-w-5xl mx-auto">
      {/* Breadcrumb */}
      <Link href="/circles" className="inline-flex items-center gap-1.5 text-xs text-sera-text-dim hover:text-sera-text transition-colors mb-4">
        <ArrowLeft size={14} /> Back to Circles
      </Link>

      {/* Header */}
      <div className="flex items-start justify-between mb-8">
        <div className="flex items-center gap-4">
          <div className="w-14 h-14 rounded-xl bg-purple-500/10 flex items-center justify-center">
            <Users size={28} className="text-purple-400" />
          </div>
          <div>
            <h1 className="sera-page-title">{circle.metadata.displayName}</h1>
            {circle.metadata.description && (
              <p className="text-sm text-sera-text-muted mt-0.5">{circle.metadata.description}</p>
            )}
            <span className="text-[11px] text-sera-text-dim font-mono">{circle.metadata.name}</span>
          </div>
        </div>
        <Link href={`/circles/${circle.metadata.name}/edit`} className="sera-btn-ghost">
          <Settings size={16} />
          Edit Circle
        </Link>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {/* Agents */}
        <div className="sera-card-static p-5">
          <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">
            Agents ({circle.agents.length})
          </h3>
          <div className="space-y-2">
            {circle.agents.map((agent) => (
              <Link
                key={agent}
                href={`/agents/${agent}`}
                className="flex items-center gap-3 p-2.5 rounded-lg hover:bg-sera-surface-hover transition-colors"
              >
                <Bot size={16} className="text-sera-accent" />
                <span className="text-sm text-sera-text">{agent}</span>
              </Link>
            ))}
            {circle.agents.length === 0 && (
              <p className="text-xs text-sera-text-dim">No agents in this circle</p>
            )}
          </div>
        </div>

        {/* Channels */}
        <div className="sera-card-static p-5">
          <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">
            Channels ({circle.channels?.length ?? 0})
          </h3>
          <div className="space-y-2">
            {circle.channels?.map((ch) => (
              <div key={ch.name} className="flex items-center justify-between py-1.5">
                <div className="flex items-center gap-2">
                  <Radio size={14} className="text-sera-text-dim" />
                  <span className="text-sm text-sera-text">{ch.name}</span>
                </div>
                <span className="sera-badge-muted">{ch.type}</span>
              </div>
            ))}
            {(!circle.channels || circle.channels.length === 0) && (
              <p className="text-xs text-sera-text-dim">No channels configured</p>
            )}
          </div>
        </div>

        {/* Party Mode */}
        {circle.partyMode && (
          <div className="sera-card-static p-5">
            <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">Party Mode</h3>
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <span className="text-xs text-sera-text-muted">Status</span>
                {circle.partyMode.enabled
                  ? <span className="sera-badge-success">Enabled</span>
                  : <span className="sera-badge-muted">Disabled</span>
                }
              </div>
              {circle.partyMode.orchestrator && (
                <div className="flex items-center justify-between">
                  <span className="text-xs text-sera-text-muted">Orchestrator</span>
                  <Link href={`/agents/${circle.partyMode.orchestrator}`} className="text-sm text-sera-accent hover:brightness-110">
                    {circle.partyMode.orchestrator}
                  </Link>
                </div>
              )}
              {circle.partyMode.selectionStrategy && (
                <div className="flex items-center justify-between">
                  <span className="text-xs text-sera-text-muted">Strategy</span>
                  <span className="sera-badge-muted">{circle.partyMode.selectionStrategy}</span>
                </div>
              )}
            </div>
          </div>
        )}

        {/* Knowledge */}
        {circle.knowledge && (
          <div className="sera-card-static p-5">
            <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">Knowledge</h3>
            <div className="flex items-center justify-between">
              <span className="text-xs text-sera-text-muted">Qdrant Collection</span>
              <span className="text-sm text-sera-text font-mono">{circle.knowledge.qdrantCollection}</span>
            </div>
          </div>
        )}
      </div>

      {/* Project Context */}
      {circle.projectContextContent && typeof circle.projectContextContent === 'string' && (
        <div className="mt-6 sera-card-static p-5">
          <div className="flex items-center gap-2 mb-3">
            <FileText size={14} className="text-sera-accent" />
            <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim">Project Context</h3>
          </div>
          <pre className="text-xs text-sera-text-muted leading-relaxed whitespace-pre-wrap font-mono bg-sera-bg/50 rounded-lg p-4 max-h-96 overflow-y-auto">
            {circle.projectContextContent}
          </pre>
        </div>
      )}
    </div>
  );
}
