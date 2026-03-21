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
  Play,
  Square,
  RotateCcw,
  Calendar,
  Clock,
  Edit2,
  Check,
  X,
  RotateCw,
} from 'lucide-react';
import Link from 'next/link';
import { toast } from 'sonner';

import {
  useAgentLogs,
  useAgentSchedules,
  useStartAgent,
  useStopAgent,
  useRestartAgent,
} from '@/hooks/useAgents';
import { useAgentBudget, usePatchAgentBudget, useResetAgentBudget } from '@/hooks/useUsage';
import { AgentStatusBadge } from '@/components/AgentStatusBadge';
import { BudgetBar } from '@/components/BudgetBar';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogClose,
} from '@/components/ui/dialog';
import { Badge } from '@/components/ui/badge';

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

type Tab = 'overview' | 'tools' | 'intercom' | 'memory' | 'logs' | 'schedules' | 'budget';

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
  const [confirmAction, setConfirmAction] = useState<'stop' | 'restart' | null>(null);

  const startAgent = useStartAgent();
  const stopAgent = useStopAgent();
  const restartAgent = useRestartAgent();

  async function handleLifecycle(action: 'start' | 'stop' | 'restart') {
    try {
      if (action === 'start') {
        await startAgent.mutateAsync(agentName);
        toast.success('Agent starting…');
      } else if (action === 'stop') {
        await stopAgent.mutateAsync(agentName);
        toast.success('Agent stopping…');
      } else {
        await restartAgent.mutateAsync(agentName);
        toast.success('Agent restarting…');
      }
    } catch (err) {
      toast.error(err instanceof Error ? err.message : `Failed to ${action}`);
    } finally {
      setConfirmAction(null);
    }
  }

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
    { id: 'logs', label: 'Logs', icon: <Bot size={15} /> },
    { id: 'schedules', label: 'Schedules', icon: <Calendar size={15} /> },
    { id: 'budget', label: 'Budget', icon: <Cpu size={15} /> },
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
        <div className="flex items-center gap-2 flex-shrink-0">
          <AgentStatusBadge agentId={agentName} />
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              void handleLifecycle('start');
            }}
            disabled={startAgent.isPending}
          >
            <Play size={13} /> Start
          </Button>
          <Button size="sm" variant="outline" onClick={() => setConfirmAction('stop')}>
            <Square size={13} /> Stop
          </Button>
          <Button size="sm" variant="outline" onClick={() => setConfirmAction('restart')}>
            <RotateCcw size={13} /> Restart
          </Button>
          <Link href={`/chat?agent=${agent.name}`} className="sera-btn-primary flex items-center gap-1.5 px-3 py-1.5 text-xs">
            <MessageSquare size={14} />
            Chat
          </Link>
          <Link href={`/agents/${agent.name}/edit`} className="sera-btn-ghost flex items-center gap-1.5 px-3 py-1.5 text-xs">
            <Settings size={14} />
            Edit
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

      {/* Logs Tab */}
      {activeTab === 'logs' && <LogsTab id={agentName} />}

      {/* Schedules Tab */}
      {activeTab === 'schedules' && <SchedulesTab id={agentName} />}

      {/* Budget Tab */}
      {activeTab === 'budget' && <BudgetTab id={agentName} />}

      {/* Confirmation dialog */}
      <Dialog
        open={confirmAction !== null}
        onOpenChange={(o: boolean) => !o && setConfirmAction(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{confirmAction === 'stop' ? 'Stop agent' : 'Restart agent'}</DialogTitle>
            <DialogDescription>
              {confirmAction === 'stop'
                ? `This will stop ${agent.displayName}. Any running tasks will be interrupted.`
                : `This will restart ${agent.displayName}. The agent will briefly go offline.`}
            </DialogDescription>
          </DialogHeader>
          <div className="flex gap-3 justify-end mt-4">
            <DialogClose asChild>
              <Button variant="ghost" size="sm">
                Cancel
              </Button>
            </DialogClose>
            <Button
              size="sm"
              variant={confirmAction === 'stop' ? 'danger' : 'outline'}
              onClick={() => {
                void handleLifecycle(confirmAction!);
              }}
            >
              {confirmAction === 'stop' ? 'Stop' : 'Restart'}
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function LogsTab({ id }: { id: string }) {
  const { data: logs, isLoading, refetch } = useAgentLogs(id);

  return (
    <div className="flex flex-col gap-3 h-full min-h-[500px]">
      <div className="flex items-center justify-between">
        <span className="text-xs text-sera-text-muted">Auto-refreshes every 3s</span>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            void refetch();
          }}
        >
          Refresh
        </Button>
      </div>
      {isLoading ? (
        <TabLoading />
      ) : (
        <pre className="flex-1 sera-card-static p-4 text-xs font-mono text-sera-text leading-relaxed overflow-auto whitespace-pre">
          {logs || 'No logs.'}
        </pre>
      )}
    </div>
  );
}

function SchedulesTab({ id }: { id: string }) {
  const { data: schedules, isLoading } = useAgentSchedules(id);

  if (isLoading) return <TabLoading />;

  if (!schedules?.length) {
    return (
      <div className="">
        <p className="text-sm text-sera-text-muted text-center py-8">No schedules configured.</p>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      {schedules.map((sched) => (
        <div key={sched.id} className="sera-card-static p-4 flex items-center gap-4">
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-1">
              <span className="font-mono text-sm text-sera-accent">{sched.cron}</span>
              {sched.description && (
                <span className="text-sm text-sera-text">{sched.description}</span>
              )}
              <Badge variant={sched.enabled ? 'success' : 'default'}>
                {sched.enabled ? 'enabled' : 'disabled'}
              </Badge>
            </div>
            <div className="flex items-center gap-4 text-xs text-sera-text-muted">
              {sched.lastRunAt && (
                <span className="flex items-center gap-1">
                  <Clock size={10} /> Last: {new Date(sched.lastRunAt).toLocaleString()}
                  {sched.lastRunStatus && (
                    <Badge variant={sched.lastRunStatus === 'success' ? 'success' : 'error'}>
                      {sched.lastRunStatus}
                    </Badge>
                  )}
                </span>
              )}
              {sched.nextRunAt && (
                <span className="flex items-center gap-1">
                  <Calendar size={10} /> Next: {new Date(sched.nextRunAt).toLocaleString()}
                </span>
              )}
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}

function BudgetTab({ id }: { id: string }) {
  const { data: budget, isLoading, refetch } = useAgentBudget(id);
  const patchBudget = usePatchAgentBudget(id);
  const resetBudget = useResetAgentBudget(id);

  const [editingHour, setEditingHour] = useState(false);
  const [editingDay, setEditingDay] = useState(false);
  const [hourVal, setHourVal] = useState('');
  const [dayVal, setDayVal] = useState('');

  const startEditHour = () => {
    setHourVal(String(budget?.maxLlmTokensPerHour ?? ''));
    setEditingHour(true);
  };

  const startEditDay = () => {
    setDayVal(String(budget?.maxLlmTokensPerDay ?? ''));
    setEditingDay(true);
  };

  const saveHour = async () => {
    const val = hourVal === '' ? null : Number(hourVal);
    try {
      await patchBudget.mutateAsync({ maxLlmTokensPerHour: val });
      toast.success('Hourly limit updated');
    } catch {
      toast.error('Failed to update hourly limit');
    }
    setEditingHour(false);
  };

  const saveDay = async () => {
    const val = dayVal === '' ? null : Number(dayVal);
    try {
      await patchBudget.mutateAsync({ maxLlmTokensPerDay: val });
      toast.success('Daily limit updated');
    } catch {
      toast.error('Failed to update daily limit');
    }
    setEditingDay(false);
  };

  const handleReset = async () => {
    try {
      await resetBudget.mutateAsync();
      toast.success('Budget counters reset');
    } catch {
      toast.error('Failed to reset budget');
    }
  };

  if (isLoading) return <TabLoading />;

  const hourPct = budget?.maxLlmTokensPerHour
    ? (budget.currentHourTokens / budget.maxLlmTokensPerHour) * 100
    : 0;
  const dayPct = budget?.maxLlmTokensPerDay
    ? (budget.currentDayTokens / budget.maxLlmTokensPerDay) * 100
    : 0;
  const exceeded = hourPct >= 100 || dayPct >= 100;

  return (
    <div className="space-y-6 max-w-xl">
      {exceeded && (
        <div className="px-4 py-3 rounded-lg bg-sera-error/10 border border-sera-error/30 text-sera-error text-sm font-medium">
          Budget exceeded — agent requests are being rejected until the period resets or the budget
          is adjusted.
        </div>
      )}

      <div className="sera-card-static p-5 space-y-5">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-sera-text">Token Budget</h3>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              void handleReset();
            }}
            disabled={resetBudget.isPending}
          >
            <RotateCw size={13} />
            Reset Counters
          </Button>
        </div>

        {/* Hourly */}
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-sera-text-muted uppercase tracking-wider">
              Hourly Limit
            </span>
            {!editingHour ? (
              <button
                onClick={startEditHour}
                className="flex items-center gap-1 text-xs text-sera-text-dim hover:text-sera-text transition-colors"
              >
                <Edit2 size={11} />
                {budget?.maxLlmTokensPerHour !== undefined
                  ? budget.maxLlmTokensPerHour.toLocaleString()
                  : 'Unlimited'}
              </button>
            ) : (
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  value={hourVal}
                  onChange={(e) => setHourVal(e.target.value)}
                  placeholder="unlimited"
                  className="sera-input text-xs w-32"
                  autoFocus
                />
                <button
                  onClick={() => {
                    void saveHour();
                  }}
                  className="text-sera-success hover:opacity-80"
                >
                  <Check size={14} />
                </button>
                <button
                  onClick={() => setEditingHour(false)}
                  className="text-sera-text-dim hover:text-sera-text"
                >
                  <X size={14} />
                </button>
              </div>
            )}
          </div>
          <BudgetBar
            label="This hour"
            current={budget?.currentHourTokens ?? 0}
            limit={budget?.maxLlmTokensPerHour}
          />
        </div>

        {/* Daily */}
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-sera-text-muted uppercase tracking-wider">
              Daily Limit
            </span>
            {!editingDay ? (
              <button
                onClick={startEditDay}
                className="flex items-center gap-1 text-xs text-sera-text-dim hover:text-sera-text transition-colors"
              >
                <Edit2 size={11} />
                {budget?.maxLlmTokensPerDay !== undefined
                  ? budget.maxLlmTokensPerDay.toLocaleString()
                  : 'Unlimited'}
              </button>
            ) : (
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  value={dayVal}
                  onChange={(e) => setDayVal(e.target.value)}
                  placeholder="unlimited"
                  className="sera-input text-xs w-32"
                  autoFocus
                />
                <button
                  onClick={() => {
                    void saveDay();
                  }}
                  className="text-sera-success hover:opacity-80"
                >
                  <Check size={14} />
                </button>
                <button
                  onClick={() => setEditingDay(false)}
                  className="text-sera-text-dim hover:text-sera-text"
                >
                  <X size={14} />
                </button>
              </div>
            )}
          </div>
          <BudgetBar
            label="Today"
            current={budget?.currentDayTokens ?? 0}
            limit={budget?.maxLlmTokensPerDay}
          />
        </div>
      </div>

      <button
        onClick={() => {
          void refetch();
        }}
        className="text-xs text-sera-text-dim hover:text-sera-text transition-colors"
      >
        Refresh usage counters
      </button>
    </div>
  );
}

function TabLoading() {
  return (
    <div className="space-y-3">
      <Skeleton className="h-6 w-full" />
      <Skeleton className="h-6 w-3/4" />
      <Skeleton className="h-6 w-1/2" />
    </div>
  );
}
