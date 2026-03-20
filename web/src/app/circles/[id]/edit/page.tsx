'use client';

import { useParams, useRouter } from 'next/navigation';
import { useState, useEffect } from 'react';
import { ArrowLeft, Save, RefreshCw, Plus, Trash2 } from 'lucide-react';
import Link from 'next/link';

export default function CircleEditPage() {
  const params = useParams();
  const router = useRouter();
  const circleName = params.id as string;

  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);

  const [form, setForm] = useState({
    displayName: '',
    description: '',
    agents: [] as string[],
    partyModeEnabled: false,
    partyOrchestrator: '',
    selectionStrategy: 'relevance',
    projectContext: '',
  });

  const [newAgent, setNewAgent] = useState('');

  useEffect(() => {
    fetch(`/api/core/circles/${circleName}`)
      .then(async (res) => {
        if (!res.ok) throw new Error('Circle not found');
        return res.json();
      })
      .then((data) => {
        setForm({
          displayName: data.metadata?.displayName || '',
          description: data.metadata?.description || '',
          agents: data.agents || [],
          partyModeEnabled: data.partyMode?.enabled ?? false,
          partyOrchestrator: data.partyMode?.orchestrator || '',
          selectionStrategy: data.partyMode?.selectionStrategy || 'relevance',
          projectContext: typeof data.projectContext === 'string' ? data.projectContext : '',
        });
      })
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [circleName]);

  const handleSave = async () => {
    setSaving(true);
    setError(null);
    setSuccess(false);

    try {
      // Save circle manifest
      const circle = {
        apiVersion: 'sera/v1',
        kind: 'Circle',
        metadata: {
          name: circleName,
          displayName: form.displayName,
          description: form.description || undefined,
        },
        agents: form.agents,
        partyMode: {
          enabled: form.partyModeEnabled,
          orchestrator: form.partyOrchestrator || undefined,
          selectionStrategy: form.selectionStrategy || undefined,
        },
      };

      const res = await fetch(`/api/core/circles/${circleName}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(circle),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error || 'Save failed');

      // Save project context if provided
      if (form.projectContext) {
        const ctxRes = await fetch(`/api/core/circles/${circleName}/context`, {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ content: form.projectContext }),
        });
        if (!ctxRes.ok) {
          const ctxData = await ctxRes.json();
          throw new Error(ctxData.error || 'Failed to save project context');
        }
      }

      setSuccess(true);
      setTimeout(() => setSuccess(false), 3000);
    } catch (err: any) {
      setError(err.message);
    } finally {
      setSaving(false);
    }
  };

  const addAgent = () => {
    const name = newAgent.trim();
    if (name && !form.agents.includes(name)) {
      setForm((prev) => ({ ...prev, agents: [...prev.agents, name] }));
      setNewAgent('');
    }
  };

  const removeAgent = (name: string) => {
    setForm((prev) => ({ ...prev, agents: prev.agents.filter((a) => a !== name) }));
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <span className="text-sm text-sera-text-muted">Loading…</span>
      </div>
    );
  }

  return (
    <div className="p-8 max-w-4xl mx-auto">
      {/* Breadcrumb */}
      <Link
        href={`/circles/${circleName}`}
        className="inline-flex items-center gap-1.5 text-xs text-sera-text-dim hover:text-sera-text transition-colors mb-4"
      >
        <ArrowLeft size={14} /> Back to {circleName}
      </Link>

      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <h1 className="sera-page-title">Edit Circle: {form.displayName || circleName}</h1>
        <button onClick={handleSave} disabled={saving} className="sera-btn-primary">
          {saving ? <RefreshCw size={14} className="animate-spin" /> : <Save size={14} />}
          Save
        </button>
      </div>

      {/* Status */}
      {error && (
        <div className="sera-card-static p-3 mb-4 border-sera-error/30 bg-sera-error/5">
          <p className="text-sm text-sera-error">{error}</p>
        </div>
      )}
      {success && (
        <div className="sera-card-static p-3 mb-4 border-sera-success/30 bg-sera-success/5">
          <p className="text-sm text-sera-success">Circle saved successfully!</p>
        </div>
      )}

      <div className="space-y-6">
        {/* Metadata */}
        <fieldset className="sera-card-static p-5">
          <legend className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim px-2">
            Metadata
          </legend>
          <div className="space-y-4 mt-2">
            <div>
              <label className="text-xs text-sera-text-muted mb-1 block">Display Name</label>
              <input
                className="sera-input"
                value={form.displayName}
                onChange={(e) => setForm((prev) => ({ ...prev, displayName: e.target.value }))}
              />
            </div>
            <div>
              <label className="text-xs text-sera-text-muted mb-1 block">Description</label>
              <textarea
                className="sera-input min-h-[60px]"
                value={form.description}
                onChange={(e) => setForm((prev) => ({ ...prev, description: e.target.value }))}
              />
            </div>
          </div>
        </fieldset>

        {/* Agents */}
        <fieldset className="sera-card-static p-5">
          <legend className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim px-2">
            Agent Roster
          </legend>
          <div className="mt-2">
            <div className="flex flex-wrap gap-2 mb-3">
              {form.agents.map((agent) => (
                <span key={agent} className="inline-flex items-center gap-1 sera-badge-accent">
                  {agent}
                  <button
                    onClick={() => removeAgent(agent)}
                    className="hover:text-sera-error transition-colors"
                  >
                    <Trash2 size={10} />
                  </button>
                </span>
              ))}
              {form.agents.length === 0 && (
                <span className="text-xs text-sera-text-dim">No agents in this circle</span>
              )}
            </div>
            <div className="flex items-center gap-2">
              <input
                className="sera-input flex-1"
                placeholder="Add agent name…"
                value={newAgent}
                onChange={(e) => setNewAgent(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), addAgent())}
              />
              <button onClick={addAgent} className="sera-btn-ghost">
                <Plus size={14} /> Add
              </button>
            </div>
          </div>
        </fieldset>

        {/* Party Mode */}
        <fieldset className="sera-card-static p-5">
          <legend className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim px-2">
            Party Mode
          </legend>
          <div className="space-y-4 mt-2">
            <div className="flex items-center gap-3">
              <label className="text-xs text-sera-text-muted">Enabled</label>
              <button
                onClick={() =>
                  setForm((prev) => ({ ...prev, partyModeEnabled: !prev.partyModeEnabled }))
                }
                className={`w-10 h-5 rounded-full transition-colors relative ${form.partyModeEnabled ? 'bg-sera-accent' : 'bg-sera-surface-hover border border-sera-border'}`}
              >
                <span
                  className={`absolute top-0.5 w-4 h-4 rounded-full bg-white transition-transform ${form.partyModeEnabled ? 'left-5' : 'left-0.5'}`}
                />
              </button>
            </div>
            {form.partyModeEnabled && (
              <>
                <div>
                  <label className="text-xs text-sera-text-muted mb-1 block">
                    Orchestrator Agent
                  </label>
                  <input
                    className="sera-input"
                    value={form.partyOrchestrator}
                    onChange={(e) =>
                      setForm((prev) => ({ ...prev, partyOrchestrator: e.target.value }))
                    }
                  />
                </div>
                <div>
                  <label className="text-xs text-sera-text-muted mb-1 block">
                    Selection Strategy
                  </label>
                  <select
                    className="sera-input"
                    value={form.selectionStrategy}
                    onChange={(e) =>
                      setForm((prev) => ({ ...prev, selectionStrategy: e.target.value }))
                    }
                  >
                    <option value="relevance">Relevance</option>
                    <option value="round-robin">Round Robin</option>
                    <option value="all">All</option>
                  </select>
                </div>
              </>
            )}
          </div>
        </fieldset>

        {/* Project Context */}
        <fieldset className="sera-card-static p-5">
          <legend className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim px-2">
            Project Context
          </legend>
          <div className="mt-2">
            <label className="text-xs text-sera-text-muted mb-1 block">
              project-context.md content
            </label>
            <textarea
              className="w-full bg-sera-bg border border-sera-border rounded-lg p-4 text-sm text-sera-text font-mono resize-y min-h-[200px] focus:outline-none focus:border-sera-border-active transition-colors"
              value={form.projectContext}
              onChange={(e) => setForm((prev) => ({ ...prev, projectContext: e.target.value }))}
              placeholder="# Project Context&#10;&#10;Write shared conventions, architecture decisions, and guidelines for this circle..."
              spellCheck={false}
            />
          </div>
        </fieldset>
      </div>
    </div>
  );
}
