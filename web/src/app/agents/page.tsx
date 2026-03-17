'use client';

import { Bot, Plus, Settings as SettingsIcon, Shield, RefreshCw, Trash2 } from 'lucide-react';
import { useState, useEffect } from 'react';
import Link from 'next/link';

interface AgentTemplate {
  name: string;
  displayName: string;
  role: string;
  tier: number;
  circle: string;
  icon: string;
}

interface AgentInstance {
  id: string;
  templateName: string;
  name: string;
  workspacePath: string;
  status: 'active' | 'inactive' | 'error';
  createdAt: string;
}

const TIER_LABELS: Record<number, { label: string; class: string }> = {
  1: { label: 'Tier 1', class: 'sera-badge-muted' },
  2: { label: 'Tier 2', class: 'sera-badge-warning' },
  3: { label: 'Tier 3', class: 'sera-badge-error' },
};

export default function AgentsPage() {
  const [templates, setTemplates] = useState<AgentTemplate[]>([]);
  const [instances, setInstances] = useState<AgentInstance[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [reloading, setReloading] = useState(false);
  const [isInstantiating, setIsInstantiating] = useState<AgentTemplate | null>(null);
  const [instanceName, setInstanceName] = useState('');
  const [isDeleting, setIsDeleting] = useState<AgentInstance | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);

  const [isDeletingTemplate, setIsDeletingTemplate] = useState<AgentTemplate | null>(null);
  const [deletingTemplateName, setDeletingTemplateName] = useState<string | null>(null);

  const fetchData = async () => {
    try {
      setLoading(true);
      const [tRes, iRes] = await Promise.all([
        fetch('/api/core/agents/templates'),
        fetch('/api/core/agents/instances')
      ]);
      
      if (!tRes.ok) throw new Error(`Failed to fetch templates: ${tRes.statusText}`);
      if (!iRes.ok) throw new Error(`Failed to fetch instances: ${iRes.statusText}`);
      
      const tData = await tRes.json();
      const iData = await iRes.json();
      
      setTemplates(tData);
      setInstances(iData);
      setError(null);
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData();
  }, []);

  const handleReload = async () => {
    setReloading(true);
    try {
      await fetch('/api/core/agents/reload', { method: 'POST' });
      await fetchData();
    } catch (err: any) {
      setError(err.message);
    } finally {
      setReloading(false);
    }
  };

  const handleInstantiate = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!isInstantiating || !instanceName.trim()) return;

    try {
      const res = await fetch('/api/core/agents/instances', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          templateName: isInstantiating.name,
          name: instanceName.trim(),
        }),
      });

      if (!res.ok) throw new Error('Failed to create instance');

      setIsInstantiating(null);
      setInstanceName('');
      fetchData();
    } catch (err: any) {
      alert(err.message);
    }
  };

  const handleDelete = async () => {
    if (!isDeleting) return;
    setDeletingId(isDeleting.id);
    try {
      const res = await fetch(`/api/core/agents/instances/${isDeleting.id}`, {
        method: 'DELETE',
      });

      if (!res.ok) throw new Error('Failed to delete instance');

      setIsDeleting(null);
      fetchData();
    } catch (err: any) {
      alert(err.message);
    } finally {
      setDeletingId(null);
    }
  };

  const handleDeleteTemplate = async () => {
    if (!isDeletingTemplate) return;
    setDeletingTemplateName(isDeletingTemplate.name);
    try {
      const res = await fetch(`/api/core/agent-templates/${isDeletingTemplate.name}`, {
        method: 'DELETE',
      });

      if (!res.ok) throw new Error('Failed to delete template');

      setIsDeletingTemplate(null);
      fetchData();
    } catch (err: any) {
      alert(err.message);
    } finally {
      setDeletingTemplateName(null);
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
          <Link href="/agents/create" className="sera-btn-primary flex items-center gap-2 px-4 py-2 text-sm">
            <Plus size={16} />
            New Agent
          </Link>
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

      {/* Active Instances */}
      {!loading && (
        <section className="mb-12">
          <h2 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-4">
            Active Instances ({instances.length})
          </h2>
          {instances.length > 0 ? (
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
              {instances.map((instance) => (
                <div
                  key={instance.id}
                  className="sera-card p-4 group border-sera-accent/20 flex flex-col justify-between"
                >
                  <div>
                    <div className="flex items-start justify-between mb-3">
                      <div className="w-10 h-10 rounded-lg bg-sera-accent-soft flex items-center justify-center text-lg">
                        {templates.find(t => t.name === instance.templateName)?.icon || '🤖'}
                      </div>
                      <span className={`sera-badge-${instance.status === 'active' ? 'accent' : 'muted'}`}>
                        {instance.status}
                      </span>
                    </div>
                    <Link href={`/chat?instance=${instance.id}`} className="block">
                      <h3 className="text-sm font-semibold text-sera-text group-hover:text-sera-accent transition-colors">
                        {instance.name}
                      </h3>
                    </Link>
                    <p className="text-[10px] text-sera-text-muted mt-1 font-mono truncate">{instance.id}</p>
                    <div className="mt-3 flex items-center gap-2">
                      <span className="text-[11px] text-sera-text-dim">
                        Template: {instance.templateName}
                      </span>
                    </div>
                  </div>
                  
                  <div className="mt-4 pt-4 border-t border-sera-border flex justify-end">
                    <button
                      onClick={(e) => {
                        e.preventDefault();
                        setIsDeleting(instance);
                      }}
                      className="p-1.5 text-sera-text-dim hover:text-sera-error hover:bg-sera-error/10 rounded-md transition-colors"
                      title="Delete instance"
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="sera-card-static p-8 text-center border-dashed border-sera-border">
              <p className="text-sm text-sera-text-muted">No active instances. Instantiate a template below to get started.</p>
            </div>
          )}
        </section>
      )}

      {/* Templates Grid */}
      {!loading && templates.length > 0 && (
        <section>
          <h2 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-4">
            Agent Templates ({templates.length})
          </h2>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
            {templates.map((template) => {
              const tierInfo = TIER_LABELS[template.tier] || TIER_LABELS[1];
              return (
                <div
                  key={template.name}
                  className="sera-card p-4 flex flex-col"
                >
                  <div className="flex items-start justify-between mb-3">
                    <div className="w-10 h-10 rounded-lg bg-sera-surface flex items-center justify-center text-lg">
                      {template.icon}
                    </div>
                    <span className={tierInfo.class}>
                      <Shield size={10} className="inline mr-0.5" />
                      {tierInfo.label}
                    </span>
                  </div>
                  <h3 className="text-sm font-semibold text-sera-text">
                    {template.displayName}
                  </h3>
                  <p className="text-xs text-sera-text-muted mt-1 line-clamp-2 flex-1">{template.role}</p>
                  
                  <div className="mt-4 pt-4 border-t border-sera-border flex items-center justify-between">
                    <span className="text-[10px] text-sera-text-dim font-mono">{template.name}</span>
                    <div className="flex items-center gap-1.5">
                      <Link
                        href={`/agents/${template.name}/edit`}
                        className="p-1.5 text-sera-text-dim hover:text-sera-accent hover:bg-sera-accent/10 rounded-md transition-colors"
                        title="Edit template"
                      >
                        <SettingsIcon size={14} />
                      </Link>
                      <button
                        onClick={() => setIsDeletingTemplate(template)}
                        className="p-1.5 text-sera-text-dim hover:text-sera-error hover:bg-sera-error/10 rounded-md transition-colors"
                        title="Delete template"
                      >
                        <Trash2 size={14} />
                      </button>
                      <button
                        onClick={() => setIsInstantiating(template)}
                        className="sera-badge-accent hover:scale-105 transition-transform cursor-pointer flex items-center gap-1 ml-1"
                      >
                        <Plus size={10} />
                        Instantiate
                      </button>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </section>
      )}

      {/* Instantiate Modal */}
      {isInstantiating && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-sera-bg/80 backdrop-blur-sm">
          <div className="sera-card-static w-full max-w-md p-6 animate-in zoom-in-95 duration-200">
            <h3 className="text-lg font-semibold text-sera-text mb-2">Instantiate {isInstantiating.displayName}</h3>
            <p className="text-sm text-sera-text-muted mb-6">Give your new agent instance a distinctive name.</p>
            
            <form onSubmit={handleInstantiate} className="space-y-4">
              <div>
                <label className="block text-xs font-semibold uppercase tracking-wider text-sera-text-dim mb-1.5">
                  Instance Name
                </label>
                <input
                  autoFocus
                  type="text"
                  value={instanceName}
                  onChange={(e) => setInstanceName(e.target.value)}
                  placeholder="e.g. Research Assistant"
                  className="w-full bg-sera-surface border border-sera-border rounded-lg px-3 py-2 text-sm text-sera-text focus:outline-none focus:border-sera-accent"
                />
              </div>
              
              <div className="flex items-center gap-3 pt-2">
                <button
                  type="button"
                  onClick={() => setIsInstantiating(null)}
                  className="flex-1 sera-btn-ghost"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  disabled={!instanceName.trim()}
                  className="flex-2 sera-btn-primary"
                >
                  Create Instance
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Delete Confirmation Modal (Instance) */}
      {isDeleting && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-sera-bg/80 backdrop-blur-sm">
          <div className="sera-card-static w-full max-w-sm p-6 animate-in zoom-in-95 duration-200">
            <div className="w-12 h-12 rounded-full bg-sera-error/10 flex items-center justify-center text-sera-error mb-4 mx-auto">
              <Trash2 size={24} />
            </div>
            <h3 className="text-lg font-semibold text-sera-text mb-2 text-center">Delete Agent Instance?</h3>
            <p className="text-sm text-sera-text-muted mb-6 text-center">
              This will stop the container for <span className="text-sera-text font-medium">"{isDeleting.name}"</span> and remove it from the database. This action cannot be undone.
            </p>
            
            <div className="flex items-center gap-3">
              <button
                type="button"
                onClick={() => setIsDeleting(null)}
                className="flex-1 sera-btn-ghost"
                disabled={!!deletingId}
              >
                Cancel
              </button>
              <button
                onClick={handleDelete}
                disabled={!!deletingId}
                className="flex-1 sera-btn-primary bg-sera-error hover:bg-sera-error/90 border-sera-error"
              >
                {deletingId ? (
                  <div className="flex items-center justify-center gap-2">
                    <RefreshCw size={14} className="animate-spin" />
                    <span>Deleting...</span>
                  </div>
                ) : (
                  'Delete'
                )}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Delete Confirmation Modal (Template) */}
      {isDeletingTemplate && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-sera-bg/80 backdrop-blur-sm">
          <div className="sera-card-static w-full max-w-sm p-6 animate-in zoom-in-95 duration-200">
            <div className="w-12 h-12 rounded-full bg-sera-error/10 flex items-center justify-center text-sera-error mb-4 mx-auto">
              <Trash2 size={24} />
            </div>
            <h3 className="text-lg font-semibold text-sera-text mb-2 text-center">Delete Agent Template?</h3>
            <p className="text-sm text-sera-text-muted mb-6 text-center">
              This will delete the <span className="text-sera-text font-medium">"AGENT.yaml"</span> manifest for <span className="text-sera-text font-medium">"{isDeletingTemplate.displayName}"</span> from disk. This action cannot be undone.
            </p>

            <div className="flex items-center gap-3">
              <button
                type="button"
                onClick={() => setIsDeletingTemplate(null)}
                className="flex-1 sera-btn-ghost"
                disabled={!!deletingTemplateName}
              >
                Cancel
              </button>
              <button
                onClick={handleDeleteTemplate}
                disabled={!!deletingTemplateName}
                className="flex-1 sera-btn-primary bg-sera-error hover:bg-sera-error/90 border-sera-error"
              >
                {deletingTemplateName ? (
                  <div className="flex items-center justify-center gap-2">
                    <RefreshCw size={14} className="animate-spin" />
                    <span>Deleting...</span>
                  </div>
                ) : (
                  'Delete'
                )}
              </button>
            </div>
          </div>
        </div>
      )}
      {/* Empty State */}
      {!loading && templates.length === 0 && !error && (
        <div className="flex flex-col items-center justify-center py-20">
          <div className="w-14 h-14 rounded-xl bg-sera-surface flex items-center justify-center mb-4">
            <Bot size={28} className="text-sera-text-dim" />
          </div>
          <h3 className="text-sm font-semibold text-sera-text mb-1">No templates found</h3>
          <p className="text-xs text-sera-text-muted text-center max-w-sm">
            Create AGENT.yaml files in the <code className="text-sera-accent">sera/agents/</code> directory.
          </p>
        </div>
      )}
    </div>
  );
}
