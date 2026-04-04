import { useState } from 'react';
import { useParams, Link } from 'react-router';
import {
  Users,
  Radio,
  Database,
  FileText,
  Trash2,
  Save,
  Settings2,
  Pencil,
  Check,
  X,
  Plus,
} from 'lucide-react';
import { useCircleDetail } from '@/hooks/useCircleDetail';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Breadcrumbs } from '@/components/Breadcrumbs';
import { CopyButton } from '@/components/CopyButton';
import { Skeleton } from '@/components/ui/skeleton';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { cn } from '@/lib/utils';
import { AddMemberDialog } from '@/components/circle/AddMemberDialog';
import { EditChannelDialog } from '@/components/circle/EditChannelDialog';
import { PartyModeDialog } from '@/components/circle/PartyModeDialog';
import { CircleOverviewTab } from '@/components/circle/CircleOverviewTab';
import { CircleChannelsTab } from '@/components/circle/CircleChannelsTab';

type Tab = 'overview' | 'channels' | 'knowledge' | 'context';

export default function CircleDetailPage() {
  const { id } = useParams<{ id: string }>();
  const [activeTab, setActiveTab] = useState<Tab>('overview');
  const c = useCircleDetail(id);

  if (c.isLoading) {
    return (
      <div className="p-6 space-y-4">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-52 rounded-xl" />
      </div>
    );
  }

  if (!c.circle) {
    return (
      <div className="p-6">
        <p className="text-sm text-sera-text-muted">Circle not found.</p>
      </div>
    );
  }

  const tabs: { key: Tab; label: string; icon: React.ReactNode }[] = [
    { key: 'overview', label: 'Overview', icon: <Settings2 size={14} /> },
    { key: 'channels', label: `Channels (${c.channels.length})`, icon: <Radio size={14} /> },
    { key: 'knowledge', label: 'Knowledge', icon: <Database size={14} /> },
    { key: 'context', label: 'Context', icon: <FileText size={14} /> },
  ];

  return (
    <div className="p-6 max-w-5xl">
      <Breadcrumbs
        items={[
          { label: 'Circles', href: '/circles' },
          { label: c.circle.displayName ?? c.circle.metadata?.displayName ?? id ?? '' },
        ]}
      />

      {/* Hero */}
      <div className="flex items-start gap-4 mb-6">
        <div className="h-14 w-14 rounded-xl bg-sera-accent-soft flex items-center justify-center flex-shrink-0">
          <Users size={24} className="text-sera-accent" />
        </div>
        <div className="flex-1 min-w-0">
          {c.editingName ? (
            <div className="flex items-center gap-2">
              <Input
                value={c.nameDraft}
                onChange={(e) => c.setNameDraft(e.target.value)}
                className="text-lg font-bold h-8 max-w-md"
                autoFocus
              />
              <button
                onClick={() => void c.handleSaveBasicInfo()}
                className="p-1.5 rounded text-sera-success hover:bg-sera-success/10"
              >
                <Check size={18} />
              </button>
              <button
                onClick={() => c.setEditingName(false)}
                className="p-1.5 rounded text-sera-text-muted hover:bg-sera-surface-hover"
              >
                <X size={18} />
              </button>
            </div>
          ) : (
            <div className="flex items-center gap-2 group/name">
              <h1 className="text-xl font-bold text-sera-text">
                {c.circle.displayName ?? c.circle.metadata?.displayName}
              </h1>
              <button
                onClick={c.startEditName}
                className="p-1 rounded text-sera-text-dim opacity-0 group-hover/name:opacity-100 hover:bg-sera-surface-hover transition-opacity"
              >
                <Pencil size={14} />
              </button>
            </div>
          )}

          <div className="flex items-center gap-1">
            <span className="text-xs text-sera-text-dim font-mono">
              {c.circle.name ?? c.circle.metadata?.name}
            </span>
            <CopyButton value={c.circle.name ?? c.circle.metadata?.name ?? id ?? ''} />
          </div>

          <div className="mt-1">
            {c.editingDesc ? (
              <div className="flex items-center gap-2">
                <Input
                  value={c.descDraft}
                  onChange={(e) => c.setDescDraft(e.target.value)}
                  className="text-xs h-7 max-w-md"
                  autoFocus
                />
                <button
                  onClick={() => void c.handleSaveBasicInfo()}
                  className="p-1 rounded text-sera-success hover:bg-sera-success/10"
                >
                  <Check size={14} />
                </button>
                <button
                  onClick={() => c.setEditingDesc(false)}
                  className="p-1 rounded text-sera-text-muted hover:bg-sera-surface-hover"
                >
                  <X size={14} />
                </button>
              </div>
            ) : (
              <div className="flex items-center gap-1.5 group/desc">
                <p className="text-sm text-sera-text-muted">
                  {c.circle.description ?? c.circle.metadata?.description ?? 'No description'}
                </p>
                <button
                  onClick={c.startEditDesc}
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
          onClick={() => c.setShowDelete(true)}
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
        <CircleOverviewTab
          agents={c.agents}
          partyMode={c.partyMode}
          connections={c.connections}
          onAddMember={() => c.setShowAddMember(true)}
          onRemoveMember={c.handleRemoveMember}
          onEditPartyMode={() => c.setShowEditParty(true)}
        />
      )}

      {activeTab === 'channels' && (
        <CircleChannelsTab
          channels={c.channels}
          onAddChannel={() =>
            c.setShowEditChannel({ index: -1, channel: { name: '', type: 'persistent' } })
          }
          onEditChannel={(i, ch) => c.setShowEditChannel({ index: i, channel: ch })}
          onDeleteChannel={c.handleDeleteChannel}
        />
      )}

      {activeTab === 'knowledge' && (
        <div>
          {!c.knowledge ? (
            <p className="text-xs text-sera-text-dim py-8 text-center">
              No knowledge configuration for this circle.
            </p>
          ) : (
            <div className="space-y-4">
              <div className="flex justify-end">
                <Button asChild size="sm" variant="outline">
                  <Link
                    to={`/memory?scope=circle&search=${encodeURIComponent(c.knowledge.qdrantCollection)}`}
                  >
                    <Database size={14} /> Browse Knowledge
                  </Link>
                </Button>
              </div>
              <div className="sera-card-static rounded-lg p-4 space-y-3">
                <div className="flex items-center gap-2">
                  <span className="text-xs text-sera-text-muted w-36">Qdrant Collection</span>
                  <span className="text-xs text-sera-text font-mono">
                    {c.knowledge.qdrantCollection}
                  </span>
                </div>
                {c.knowledge.postgresSchema && (
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-sera-text-muted w-36">Postgres Schema</span>
                    <span className="text-xs text-sera-text font-mono">
                      {c.knowledge.postgresSchema}
                    </span>
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      )}

      {activeTab === 'context' && (
        <div>
          {c.editingContext ? (
            <div className="space-y-3">
              <textarea
                value={c.contextDraft}
                onChange={(e) => c.setContextDraft(e.target.value)}
                className="sera-input w-full min-h-[300px] font-mono text-xs p-3 rounded-lg resize-y"
                placeholder="Write project context in markdown…"
              />
              <div className="flex items-center gap-2">
                <Button
                  size="sm"
                  onClick={() => void c.handleSaveContext()}
                  disabled={c.savingContext}
                >
                  <Save size={13} />
                  {c.savingContext ? 'Saving…' : 'Save'}
                </Button>
                <Button variant="ghost" size="sm" onClick={() => c.setEditingContext(false)}>
                  Cancel
                </Button>
              </div>
            </div>
          ) : c.projectContent ? (
            <div>
              <div className="flex items-center justify-between mb-3">
                <span className="text-xs text-sera-text-muted">Project context (markdown)</span>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => c.startEditContext(c.projectContent)}
                >
                  <Pencil size={12} />
                  Edit
                </Button>
              </div>
              <pre className="sera-card-static rounded-lg p-4 text-xs text-sera-text whitespace-pre-wrap font-mono leading-relaxed max-h-[500px] overflow-y-auto">
                {c.projectContent}
              </pre>
            </div>
          ) : (
            <div className="text-center py-8">
              <p className="text-xs text-sera-text-dim mb-3">
                No project context set for this circle.
              </p>
              <Button variant="ghost" size="sm" onClick={() => c.startEditContext()}>
                <Plus size={13} />
                Add Context
              </Button>
            </div>
          )}
        </div>
      )}

      {/* Dialogs */}
      <AddMemberDialog
        open={c.showAddMember}
        onOpenChange={c.setShowAddMember}
        allAgents={c.allAgents}
        currentMemberIds={c.agents}
        selectedAgents={c.selectedAgentsForCircle}
        onSelectedAgentsChange={c.setSelectedAgentsForCircle}
        onMembersAdded={() => void c.handleAddMembers()}
        isLoading={c.isPending}
      />

      <PartyModeDialog
        open={c.showEditParty}
        onOpenChange={c.setShowEditParty}
        partyMode={c.partyMode}
        agents={c.agents}
        onSave={c.handleSavePartyMode}
        isLoading={c.isPending}
      />

      <EditChannelDialog
        open={!!c.showEditChannel}
        onOpenChange={(o) => !o && c.setShowEditChannel(null)}
        channelData={c.showEditChannel}
        onChannelDataChange={c.setShowEditChannel}
        onSave={c.handleSaveChannel}
        isLoading={c.isPending}
      />

      <Dialog open={c.showDelete} onOpenChange={c.setShowDelete}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Circle</DialogTitle>
            <DialogDescription>
              This will permanently remove the circle manifest from disk. Agents will be unaffected
              but will lose their circle membership.
            </DialogDescription>
          </DialogHeader>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" size="sm" onClick={() => c.setShowDelete(false)}>
              Cancel
            </Button>
            <Button
              variant="danger"
              size="sm"
              onClick={() => void c.handleDelete()}
              disabled={c.isDeletePending}
            >
              <Trash2 size={13} />
              {c.isDeletePending ? 'Deleting…' : 'Delete Circle'}
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
