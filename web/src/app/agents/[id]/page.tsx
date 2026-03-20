'use client';

import { useParams } from 'next/navigation';
import { useState, useEffect } from 'react';
import {
  Bot,
  ArrowLeft,
  Shield,
  Settings,
  BookOpen,
  Cpu,
  MessageSquare,
  Wrench,
  Users,
} from 'lucide-react';
import Link from 'next/link';

interface MemoryEntry {
  id: string;
  title: string;
  type: string;
  content: string;
  refs: string[];
  tags: string[];
  source: string;
  createdAt: string;
  updatedAt: string;
}

interface MemoryBlock {
  type: string;
  entries: MemoryEntry[];
}

interface AgentDetail {
  name: string;
  displayName: string;
  role: string;
  tier: number;
  circle: string;
  icon: string;
  manifest: {
    apiVersion: string;
    kind: string;
    metadata: Record<string, any>;
    identity: {
      role: string;
      description: string;
      communicationStyle?: string;
      principles?: string[];
    };
    model: {
      provider: string;
      name: string;
      temperature?: number;
      fallback?: Array<{ provider: string; name: string; maxComplexity?: number }>;
    };
    tools?: { allowed?: string[]; denied?: string[] };
    skills?: string[];
    subagents?: {
      allowed?: Array<{ role: string; maxInstances?: number; requiresApproval?: boolean }>;
    };
    intercom?: {
      canMessage?: string[];
      channels?: { publish?: string[]; subscribe?: string[] };
    };
    resources?: { memory?: string; cpu?: string };
    workspace?: { provider?: string; path?: string };
    memory?: { personalMemory?: string; sharedKnowledge?: string };
  };
}

type Tab = 'overview' | 'tools' | 'intercom' | 'memory';

const TIER_LABELS: Record<number, { label: string; class: string; desc: string }> = {
  1: {
    label: 'Tier 1 — Restricted',
    class: 'sera-badge-muted',
    desc: 'Read-only filesystem, no network',
  },
  2: {
    label: 'Tier 2 — Standard',
    class: 'sera-badge-warning',
    desc: 'Read-write workspace, SERA network',
  },
  3: {
    label: 'Tier 3 — Privileged',
    class: 'sera-badge-error',
    desc: 'Full access, bridged network',
  },
};

export default function AgentDetailPage() {
  const params = useParams();
  const agentName = params.id as string;
  const [agent, setAgent] = useState<AgentDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<Tab>('overview');
  const [memoryBlocks, setMemoryBlocks] = useState<MemoryBlock[]>([]);
  const [loadingMemory, setLoadingMemory] = useState(false);

  useEffect(() => {
    fetch(`/api/core/agents/${agentName}`)
      .then(async (res) => {
        if (!res.ok) throw new Error(`Agent not found`);
        return res.json();
      })
      .then(setAgent)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));

    setLoadingMemory(true);
    fetch('/api/core/memory/blocks')
      .then(async (res) => {
        if (!res.ok) throw new Error('Failed to fetch memory');
        return res.json();
      })
      .then(setMemoryBlocks)
      .catch((err) => console.error('Error fetching memory:', err))
      .finally(() => setLoadingMemory(false));
  }, [agentName]);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <span className="text-sm text-sera-text-muted">Loading…</span>
      </div>
    );
  }

  if (error || !agent) {
    return (
      <div className="p-8 max-w-5xl mx-auto">
        <Link
          href="/agents"
          className="inline-flex items-center gap-1.5 text-xs text-sera-text-dim hover:text-sera-text transition-colors mb-4"
        >
          <ArrowLeft size={14} /> Back to Agents
        </Link>
        <div className="sera-card-static p-6 text-center">
          <Bot size={32} className="text-sera-text-dim mx-auto mb-3" />
          <p className="text-sm text-sera-error">{error || 'Agent not found'}</p>
        </div>
      </div>
    );
  }

  const m = agent.manifest;
  const tierInfo = TIER_LABELS[agent.tier] || TIER_LABELS[1];

  const tabs: { id: Tab; label: string; icon: React.ReactNode }[] = [
    { id: 'overview', label: 'Overview', icon: <Cpu size={15} /> },
    { id: 'tools', label: 'Tools & Skills', icon: <Wrench size={15} /> },
    { id: 'intercom', label: 'Intercom', icon: <MessageSquare size={15} /> },
    { id: 'memory', label: 'Memory', icon: <BookOpen size={15} /> },
  ];

  return (
    <div className="p-8 max-w-5xl mx-auto">
      {/* Breadcrumb */}
      <Link
        href="/agents"
        className="inline-flex items-center gap-1.5 text-xs text-sera-text-dim hover:text-sera-text transition-colors mb-4"
      >
        <ArrowLeft size={14} /> Back to Agents
      </Link>

      {/* Header */}
      <div className="flex items-start justify-between mb-8">
        <div className="flex items-center gap-4">
          <div className="w-14 h-14 rounded-xl bg-sera-accent-soft flex items-center justify-center text-2xl">
            {agent.icon}
          </div>
          <div>
            <div className="flex items-center gap-3">
              <h1 className="sera-page-title">{agent.displayName}</h1>
              <span className={tierInfo.class}>
                <Shield size={10} className="inline mr-0.5" />
                {tierInfo.label}
              </span>
            </div>
            <p className="text-sm text-sera-text-muted mt-0.5">{agent.role}</p>
            <div className="flex items-center gap-2 mt-1.5">
              <span className="sera-badge-accent">{agent.circle}</span>
              <span className="text-[11px] text-sera-text-dim font-mono">{agent.name}</span>
            </div>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Link href={`/chat?agent=${agent.name}`} className="sera-btn-primary">
            <MessageSquare size={16} />
            Chat with this agent
          </Link>
          <Link href={`/agents/${agent.name}/edit`} className="sera-btn-ghost">
            <Settings size={16} />
            Edit Manifest
          </Link>
        </div>
      </div>

      {/* Tabs */}
      <div className="flex items-center gap-1 border-b border-sera-border mb-6">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`flex items-center gap-2 px-4 py-3 text-sm font-medium border-b-2 transition-colors duration-150
              ${
                activeTab === tab.id
                  ? 'border-sera-accent text-sera-accent'
                  : 'border-transparent text-sera-text-muted hover:text-sera-text'
              }`}
          >
            {tab.icon}
            {tab.label}
          </button>
        ))}
      </div>

      {/* Overview Tab */}
      {activeTab === 'overview' && (
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
          {/* Identity */}
          <div className="sera-card-static p-5">
            <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">
              Identity
            </h3>
            <p className="text-sm text-sera-text leading-relaxed mb-3">{m.identity.description}</p>
            {m.identity.communicationStyle && (
              <div className="mb-3">
                <span className="text-[11px] text-sera-text-dim uppercase tracking-wide">
                  Communication Style
                </span>
                <p className="text-xs text-sera-text-muted mt-1">{m.identity.communicationStyle}</p>
              </div>
            )}
            {m.identity.principles && m.identity.principles.length > 0 && (
              <div>
                <span className="text-[11px] text-sera-text-dim uppercase tracking-wide">
                  Principles
                </span>
                <ul className="mt-1 space-y-1">
                  {m.identity.principles.map((p, i) => (
                    <li key={i} className="text-xs text-sera-text-muted flex items-start gap-1.5">
                      <span className="text-sera-accent mt-0.5">•</span>
                      {p}
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </div>

          {/* Model */}
          <div className="sera-card-static p-5">
            <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">
              Model Configuration
            </h3>
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <span className="text-xs text-sera-text-muted">Provider</span>
                <span className="sera-badge-accent">{m.model.provider}</span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-xs text-sera-text-muted">Model</span>
                <span className="text-sm text-sera-text font-mono">{m.model.name}</span>
              </div>
              {m.model.temperature !== undefined && (
                <div className="flex items-center justify-between">
                  <span className="text-xs text-sera-text-muted">Temperature</span>
                  <span className="text-sm text-sera-text">{m.model.temperature}</span>
                </div>
              )}
              {m.model.fallback && m.model.fallback.length > 0 && (
                <div className="mt-2 pt-2 border-t border-sera-border">
                  <span className="text-[11px] text-sera-text-dim uppercase tracking-wide">
                    Fallback Models
                  </span>
                  {m.model.fallback.map((fb, i) => (
                    <div key={i} className="flex items-center justify-between mt-1.5">
                      <span className="text-xs text-sera-text-muted font-mono">
                        {fb.provider}/{fb.name}
                      </span>
                      {fb.maxComplexity && (
                        <span className="text-[11px] text-sera-text-dim">
                          max complexity: {fb.maxComplexity}
                        </span>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>

          {/* Resources */}
          <div className="sera-card-static p-5">
            <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">
              Resources
            </h3>
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <span className="text-xs text-sera-text-muted">Security Tier</span>
                <span className={tierInfo.class}>{tierInfo.label}</span>
              </div>
              <p className="text-[11px] text-sera-text-dim">{tierInfo.desc}</p>
              {m.resources && (
                <>
                  <div className="flex items-center justify-between">
                    <span className="text-xs text-sera-text-muted">Memory Limit</span>
                    <span className="text-sm text-sera-text font-mono">
                      {m.resources.memory || '—'}
                    </span>
                  </div>
                  <div className="flex items-center justify-between">
                    <span className="text-xs text-sera-text-muted">CPU Limit</span>
                    <span className="text-sm text-sera-text font-mono">
                      {m.resources.cpu || '—'}
                    </span>
                  </div>
                </>
              )}
            </div>
          </div>

          {/* Workspace & Memory */}
          <div className="sera-card-static p-5">
            <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">
              Workspace & Memory
            </h3>
            <div className="space-y-3">
              {m.workspace && (
                <>
                  <div className="flex items-center justify-between">
                    <span className="text-xs text-sera-text-muted">Storage Provider</span>
                    <span className="sera-badge-muted">{m.workspace.provider || 'default'}</span>
                  </div>
                  <div className="flex items-center justify-between">
                    <span className="text-xs text-sera-text-muted">Workspace Path</span>
                    <span className="text-xs text-sera-text font-mono truncate max-w-[200px]">
                      {m.workspace.path || '—'}
                    </span>
                  </div>
                </>
              )}
              {m.memory && (
                <>
                  <div className="flex items-center justify-between">
                    <span className="text-xs text-sera-text-muted">Personal Memory</span>
                    <span className="text-xs text-sera-text font-mono truncate max-w-[200px]">
                      {m.memory.personalMemory || '—'}
                    </span>
                  </div>
                  <div className="flex items-center justify-between">
                    <span className="text-xs text-sera-text-muted">Shared Knowledge</span>
                    <span className="text-xs text-sera-text font-mono truncate max-w-[200px]">
                      {m.memory.sharedKnowledge || '—'}
                    </span>
                  </div>
                </>
              )}
            </div>
          </div>
        </div>
      )}

      {/* Tools & Skills Tab */}
      {activeTab === 'tools' && (
        <div className="space-y-6">
          {/* Allowed Tools */}
          <div className="sera-card-static p-5">
            <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">
              Allowed Tools
            </h3>
            <div className="flex flex-wrap gap-2">
              {m.tools?.allowed?.map((tool) => (
                <span key={tool} className="sera-badge-accent">
                  {tool}
                </span>
              )) || <span className="text-xs text-sera-text-dim">No tools configured</span>}
            </div>
          </div>

          {/* Denied Tools */}
          {m.tools?.denied && m.tools.denied.length > 0 && (
            <div className="sera-card-static p-5">
              <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">
                Denied Tools
              </h3>
              <div className="flex flex-wrap gap-2">
                {m.tools.denied.map((tool) => (
                  <span key={tool} className="sera-badge-error">
                    {tool}
                  </span>
                ))}
              </div>
            </div>
          )}

          {/* Skills */}
          <div className="sera-card-static p-5">
            <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">
              Skills
            </h3>
            <div className="flex flex-wrap gap-2">
              {m.skills?.map((skill) => (
                <span key={skill} className="sera-badge bg-purple-500/15 text-purple-400">
                  {skill}
                </span>
              )) || <span className="text-xs text-sera-text-dim">No skills configured</span>}
            </div>
          </div>

          {/* Subagents */}
          {m.subagents?.allowed && m.subagents.allowed.length > 0 && (
            <div className="sera-card-static p-5">
              <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">
                Allowed Subagents
              </h3>
              <div className="space-y-2">
                {m.subagents.allowed.map((sa) => (
                  <div key={sa.role} className="flex items-center justify-between py-1.5">
                    <div className="flex items-center gap-2">
                      <Users size={14} className="text-sera-text-dim" />
                      <span className="text-sm text-sera-text">{sa.role}</span>
                    </div>
                    <div className="flex items-center gap-2">
                      {sa.maxInstances && (
                        <span className="text-[11px] text-sera-text-dim">
                          max: {sa.maxInstances}
                        </span>
                      )}
                      {sa.requiresApproval && (
                        <span className="sera-badge-warning">Approval Required</span>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      {/* Memory Tab */}
      {activeTab === 'memory' && (
        <div className="space-y-6">
          {loadingMemory ? (
            <div className="flex items-center justify-center py-20">
              <span className="text-sm text-sera-text-muted">Loading memory...</span>
            </div>
          ) : memoryBlocks.length === 0 ? (
            <div className="sera-card-static p-8 text-center">
              <BookOpen size={24} className="text-sera-text-dim mx-auto mb-3" />
              <p className="text-sm text-sera-text-muted">No memory blocks found.</p>
            </div>
          ) : (
            memoryBlocks.map((block) => (
              <div key={block.type} className="sera-card-static p-5">
                <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-4">
                  {block.type} Block ({block.entries.length})
                </h3>
                {block.entries.length === 0 ? (
                  <p className="text-xs text-sera-text-dim italic">No entries</p>
                ) : (
                  <div className="space-y-3">
                    {block.entries.map((entry) => (
                      <div
                        key={entry.id}
                        className="border border-sera-border rounded-lg p-3 bg-sera-bg/30"
                      >
                        <div className="flex items-center justify-between mb-2">
                          <h4 className="text-sm font-medium text-sera-text">{entry.title}</h4>
                          <span className="text-[10px] text-sera-text-dim font-mono">
                            {new Date(entry.createdAt).toLocaleString()}
                          </span>
                        </div>
                        <p className="text-xs text-sera-text-muted whitespace-pre-wrap">
                          {entry.content}
                        </p>
                        {entry.tags && entry.tags.length > 0 && (
                          <div className="mt-2 flex flex-wrap gap-1">
                            {entry.tags.map((tag) => (
                              <span
                                key={tag}
                                className="text-[10px] bg-sera-surface px-1.5 py-0.5 rounded text-sera-text-dim"
                              >
                                #{tag}
                              </span>
                            ))}
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            ))
          )}
        </div>
      )}

      {/* Intercom Tab */}
      {activeTab === 'intercom' && (
        <div className="space-y-6">
          {/* Can Message */}
          <div className="sera-card-static p-5">
            <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">
              Can Message
            </h3>
            <div className="flex flex-wrap gap-2">
              {m.intercom?.canMessage?.map((peer) => (
                <Link
                  key={peer}
                  href={`/agents/${peer}`}
                  className="sera-badge-accent hover:brightness-110 transition-all"
                >
                  {peer}
                </Link>
              )) || <span className="text-xs text-sera-text-dim">No peers configured</span>}
            </div>
          </div>

          {/* Channels */}
          {m.intercom?.channels && (
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
              <div className="sera-card-static p-5">
                <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">
                  Publish Channels
                </h3>
                <div className="flex flex-wrap gap-2">
                  {m.intercom.channels.publish?.map((ch) => (
                    <span key={ch} className="sera-badge bg-emerald-500/15 text-emerald-400">
                      {ch}
                    </span>
                  )) || <span className="text-xs text-sera-text-dim">None</span>}
                </div>
              </div>
              <div className="sera-card-static p-5">
                <h3 className="text-xs font-semibold uppercase tracking-[0.1em] text-sera-text-dim mb-3">
                  Subscribe Channels
                </h3>
                <div className="flex flex-wrap gap-2">
                  {m.intercom.channels.subscribe?.map((ch) => (
                    <span key={ch} className="sera-badge bg-blue-500/15 text-blue-400">
                      {ch}
                    </span>
                  )) || <span className="text-xs text-sera-text-dim">None</span>}
                </div>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
