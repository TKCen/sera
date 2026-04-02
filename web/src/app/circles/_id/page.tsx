import { useState } from 'react';
import { useParams, useNavigate, Link } from 'react-router';
import {
  Users,
  ArrowLeft,
  Bot,
  Radio,
  Zap,
  Database,
  FileText,
  Network,
  Trash2,
  Save,
  Settings2,
  Pencil,
  Check,
  X,
  Plus,
} from 'lucide-react';
import { toast } from 'sonner';
import { useCircle, useUpdateCircle, useDeleteCircle, useUpdateCircleContext } from '@/hooks/useCircles';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { cn } from '@/lib/utils';

type Tab = 'overview' | 'channels' | 'knowledge' | 'context';

export default function CircleDetailPage() {
  const { id = '' } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { data: circle, isLoading } = useCircle(id);
  const updateCircle = useUpdateCircle();
  const deleteCircle = useDeleteCircle();
  const updateCircleContext = useUpdateCircleContext();

  const [activeTab, setActiveTab] = useState<Tab>('overview');
  const [showDelete, setShowDelete] = useState(false);

  // Inline editing for description
  const [editingDesc, setEditingDesc] = useState(false);
  const [descDraft, setDescDraft] = useState('');

  // Project context editing
  const [editingContext, setEditingContext] = useState(false);
  const [contextDraft, setContextDraft] = useState('');
  const [savingContext, setSavingContext] = useState(false);

  if (isLoading) {
    return (
      <div className="p-6 space-y-4">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-52 rounded-xl" />
      </div>
    );
  }

  if (!circle) {
    return (
      <div className="p-6">
        <p className="text-sm text-sera-text-muted">Circle not found.</p>
      </div>
    );
  }

  const agents = circle.agents ?? [];
  const channels = circle.channels ?? [];
  const connections = circle.connections ?? [];
  const partyMode = circle.partyMode;
  const knowledge = circle.knowledge;
  // DB circles have `constitution`, YAML circles have `projectContext.content`
  const projectContent =
    ((circle as unknown as Record<string, unknown>).constitution as string | undefined) ??
    (typeof circle.projectContext === 'object' && circle.projectContext !== null
      ? ((circle.projectContext as Record<string, unknown>).content as string | undefined)
      : typeof circle.projectContext === 'string'
        ? circle.projectContext
        : undefined);

  async function handleDelete() {
    if (!id) return;
    try {
      await deleteCircle.mutateAsync(id);
      toast.success(
        `Deleted circle "${circle?.displayName ?? circle?.metadata?.displayName ?? id}"`
      );
      void navigate('/circles');
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to delete');
    }
  }

  async function handleSaveDescription() {
    if (!id || !circle) return;
    try {
      await updateCircle.mutateAsync({
        name: id,
        manifest: {
          ...circle,
          metadata: {
            name: circle.name ?? circle.metadata?.name ?? id,
            displayName: circle.displayName ?? circle.metadata?.displayName ?? id,
            description: descDraft.trim() || undefined,
          },
        },
      });
      toast.success('Description updated');
      setEditingDesc(false);
      void refetch();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to update');
    }
  }

  async function handleSaveContext() {
    if (!id) return;
    setSavingContext(true);
    try {
      await updateCircleContext.mutateAsync({ name: id, context: contextDraft });
      toast.success('Project context saved');
      setEditingContext(false);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to save context');
    } finally {
      setSavingContext(false);
    }
  }

  const tabs: { key: Tab; label: string; icon: React.ReactNode }[] = [
    { key: 'overview', label: 'Overview', icon: <Settings2 size={14} /> },
    { key: 'channels', label: `Channels (${channels.length})`, icon: <Radio size={14} /> },
    { key: 'knowledge', label: 'Knowledge', icon: <Database size={14} /> },
    { key: 'context', label: 'Context', icon: <FileText size={14} /> },
  ];

  return (
    <div className="p-6 max-w-5xl">
      {/* Back link */}
      <Link
        to="/circles"
        className="inline-flex items-center gap-1.5 text-xs text-sera-text-muted hover:text-sera-text transition-colors mb-4"
      >
        <ArrowLeft size={12} />
        Circles
      </Link>

      {/* Hero */}
      <div className="flex items-start gap-4 mb-6">
        <div className="h-14 w-14 rounded-xl bg-sera-accent-soft flex items-center justify-center flex-shrink-0">
          <Users size={24} className="text-sera-accent" />
        </div>
        <div className="flex-1 min-w-0">
          <h1 className="text-xl font-bold text-sera-text">
            {circle.displayName ?? circle.metadata?.displayName}
          </h1>
          <span className="text-xs text-sera-text-dim font-mono">
            {circle.name ?? circle.metadata?.name}
          </span>

          {/* Description — inline editable */}
          <div className="mt-1">
            {editingDesc ? (
              <div className="flex items-center gap-2">
                <Input
                  value={descDraft}
                  onChange={(e) => setDescDraft(e.target.value)}
                  className="text-xs h-7 max-w-md"
                  autoFocus
                />
                <button
                  onClick={() => {
                    void handleSaveDescription();
                  }}
                  className="p-1 rounded text-sera-success hover:bg-sera-success/10"
                >
                  <Check size={14} />
                </button>
                <button
                  onClick={() => setEditingDesc(false)}
                  className="p-1 rounded text-sera-text-muted hover:bg-sera-surface-hover"
                >
                  <X size={14} />
                </button>
              </div>
            ) : (
              <div className="flex items-center gap-1.5 group/desc">
                <p className="text-sm text-sera-text-muted">
                  {circle.description ?? circle.metadata?.description ?? 'No description'}
                </p>
                <button
                  onClick={() => {
                    setDescDraft(circle.description ?? circle.metadata?.description ?? '');
                    setEditingDesc(true);
                  }}
                  className="p-1 rounded text-sera-text-dim opacity-0 group-hover/desc:opacity-100 hover:bg-sera-surface-hover transition-opacity"
                  title="Edit description"
                >
                  <Pencil size={12} />
                </button>
              </div>
            )}
          </div>
        </div>

        <Button
          variant="danger"
          size="sm"
          onClick={() => setShowDelete(true)}
          className="flex-shrink-0"
        >
          <Trash2 size={13} />
          Delete
        </Button>
      </div>

      {/* Tabs */}
      <div className="flex items-center gap-1 border-b border-sera-border mb-6 pb-px">
        {tabs.map((tab) => (
          <button
            key={tab.key}
            onClick={() => setActiveTab(tab.key)}
            className={cn(
              'flex items-center gap-1.5 px-3 py-2 text-xs font-medium rounded-t-md transition-colors -mb-px',
              activeTab === tab.key
                ? 'text-sera-accent border-b-2 border-sera-accent bg-sera-accent-soft/30'
                : 'text-sera-text-muted hover:text-sera-text hover:bg-sera-surface-hover'
            )}
          >
            {tab.icon}
            {tab.label}
          </button>
        ))}
      </div>

      {/* Tab Content */}
      {activeTab === 'overview' && (
        <div className="space-y-6">
          {/* Agents membership */}
          <section>
            <h2 className="text-sm font-semibold text-sera-text mb-3 flex items-center gap-2">
              <Bot size={15} />
              Members ({agents.length})
            </h2>
            {agents.length === 0 ? (
              <p className="text-xs text-sera-text-dim">No agents in this circle.</p>
            ) : (
              <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                {agents.map((agent) => (
                  <Link
                    key={agent}
                    to={`/agents?search=${encodeURIComponent(agent)}`}
                    className="sera-card-static rounded-lg px-4 py-3 flex items-center gap-3 hover:bg-sera-surface-hover transition-colors"
                  >
                    <div className="h-8 w-8 rounded-full bg-sera-accent-soft flex items-center justify-center flex-shrink-0">
                      <Bot size={14} className="text-sera-accent" />
                    </div>
                    <div className="min-w-0">
                      <span className="text-sm font-medium text-sera-text truncate block">
                        {agent}
                      </span>
                    </div>
                  </Link>
                ))}
              </div>
            )}
          </section>

          {/* Party Mode */}
          {partyMode && (
            <section>
              <h2 className="text-sm font-semibold text-sera-text mb-3 flex items-center gap-2">
                <Zap size={15} />
                Party Mode
              </h2>
              <div className="sera-card-static rounded-lg p-4 space-y-2">
                <div className="flex items-center gap-2">
                  <span className="text-xs text-sera-text-muted w-28">Status</span>
                  <Badge variant={partyMode.enabled ? 'success' : 'default'}>
                    {partyMode.enabled ? 'Enabled' : 'Disabled'}
                  </Badge>
                </div>
                {partyMode.orchestrator && (
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-sera-text-muted w-28">Orchestrator</span>
                    <span className="text-xs text-sera-text font-mono">
                      {partyMode.orchestrator}
                    </span>
                  </div>
                )}
                {partyMode.selectionStrategy && (
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-sera-text-muted w-28">Strategy</span>
                    <Badge variant="accent">{partyMode.selectionStrategy}</Badge>
                  </div>
                )}
              </div>
            </section>
          )}

          {/* Connections */}
          {connections.length > 0 && (
            <section>
              <h2 className="text-sm font-semibold text-sera-text mb-3 flex items-center gap-2">
                <Network size={15} />
                Connections ({connections.length})
              </h2>
              <div className="space-y-2">
                {connections.map((conn, i) => (
                  <div
                    key={i}
                    className="sera-card-static rounded-lg px-4 py-3 flex items-center gap-3"
                  >
                    <Network size={14} className="text-sera-text-muted flex-shrink-0" />
                    <div className="flex-1 min-w-0">
                      <Link
                        to={`/circles/${conn.circle}`}
                        className="text-sm font-medium text-sera-accent hover:underline"
                      >
                        {conn.circle}
                      </Link>
                      {conn.bridgeChannels && conn.bridgeChannels.length > 0 && (
                        <div className="flex items-center gap-1 mt-1 flex-wrap">
                          {conn.bridgeChannels.map((ch) => (
                            <Badge key={ch} variant="default" className="text-[10px]">
                              {ch}
                            </Badge>
                          ))}
                        </div>
                      )}
                    </div>
                    <Badge variant="default">
                      {typeof conn.auth === 'string' ? conn.auth : 'custom'}
                    </Badge>
                  </div>
                ))}
              </div>
            </section>
          )}
        </div>
      )}

      {activeTab === 'channels' && (
        <div>
          {channels.length === 0 ? (
            <p className="text-xs text-sera-text-dim py-8 text-center">
              No channels configured for this circle.
            </p>
          ) : (
            <div className="space-y-2">
              {channels.map((ch, i) => (
                <div
                  key={ch.id ?? ch.name ?? i}
                  className="sera-card-static rounded-lg px-4 py-3 flex items-center gap-3"
                >
                  <Radio size={14} className="text-sera-text-muted flex-shrink-0" />
                  <div className="flex-1 min-w-0">
                    <span className="text-sm font-medium text-sera-text">{ch.name}</span>
                    {ch.description && (
                      <p className="text-xs text-sera-text-dim mt-0.5">{ch.description}</p>
                    )}
                  </div>
                  {ch.type && (
                    <Badge variant={ch.type === 'persistent' ? 'accent' : 'warning'}>
                      {ch.type}
                    </Badge>
                  )}
                  {ch.id && (
                    <span className="text-[10px] text-sera-text-dim font-mono">{ch.id}</span>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {activeTab === 'knowledge' && (
        <div>
          {!knowledge ? (
            <p className="text-xs text-sera-text-dim py-8 text-center">
              No knowledge configuration for this circle.
            </p>
          ) : (
            <div className="sera-card-static rounded-lg p-4 space-y-3">
              <div className="flex items-center gap-2">
                <span className="text-xs text-sera-text-muted w-36">Qdrant Collection</span>
                <span className="text-xs text-sera-text font-mono">
                  {knowledge.qdrantCollection}
                </span>
              </div>
              {knowledge.postgresSchema && (
                <div className="flex items-center gap-2">
                  <span className="text-xs text-sera-text-muted w-36">Postgres Schema</span>
                  <span className="text-xs text-sera-text font-mono">
                    {knowledge.postgresSchema}
                  </span>
                </div>
              )}
            </div>
          )}
        </div>
      )}

      {activeTab === 'context' && (
        <div>
          {editingContext ? (
            <div className="space-y-3">
              <textarea
                value={contextDraft}
                onChange={(e) => setContextDraft(e.target.value)}
                className="sera-input w-full min-h-[300px] font-mono text-xs p-3 rounded-lg resize-y"
                placeholder="Write project context in markdown…"
              />
              <div className="flex items-center gap-2">
                <Button
                  size="sm"
                  onClick={() => {
                    void handleSaveContext();
                  }}
                  disabled={savingContext}
                >
                  <Save size={13} />
                  {savingContext ? 'Saving…' : 'Save'}
                </Button>
                <Button variant="ghost" size="sm" onClick={() => setEditingContext(false)}>
                  Cancel
                </Button>
              </div>
            </div>
          ) : projectContent ? (
            <div>
              <div className="flex items-center justify-between mb-3">
                <span className="text-xs text-sera-text-muted">Project context (markdown)</span>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => {
                    setContextDraft(projectContent);
                    setEditingContext(true);
                  }}
                >
                  <Pencil size={12} />
                  Edit
                </Button>
              </div>
              <pre className="sera-card-static rounded-lg p-4 text-xs text-sera-text whitespace-pre-wrap font-mono leading-relaxed max-h-[500px] overflow-y-auto">
                {projectContent}
              </pre>
            </div>
          ) : (
            <div className="text-center py-8">
              <p className="text-xs text-sera-text-dim mb-3">
                No project context set for this circle.
              </p>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  setContextDraft('');
                  setEditingContext(true);
                }}
              >
                <Plus size={13} />
                Add Context
              </Button>
            </div>
          )}
        </div>
      )}

      {/* Delete confirmation */}
      <Dialog open={showDelete} onOpenChange={setShowDelete}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Circle</DialogTitle>
            <DialogDescription>
              This will permanently remove the circle manifest from disk. Agents will be unaffected
              but will lose their circle membership.
            </DialogDescription>
          </DialogHeader>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" size="sm" onClick={() => setShowDelete(false)}>
              Cancel
            </Button>
            <Button
              variant="danger"
              size="sm"
              onClick={() => {
                void handleDelete();
              }}
              disabled={deleteCircle.isPending}
            >
              <Trash2 size={13} />
              {deleteCircle.isPending ? 'Deleting…' : 'Delete Circle'}
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
