import { useState } from 'react';
import { useNavigate } from 'react-router';
import { toast } from 'sonner';
import { useCircle, useUpdateCircle, useDeleteCircle } from '@/hooks/useCircles';
import { useAgents } from '@/hooks/useAgents';
import * as circlesApi from '@/lib/api/circles';
import type { CircleChannelConfig, CirclePartyModeConfig } from '@/lib/api/types';

export function useCircleDetail(id: string | undefined) {
  const navigate = useNavigate();
  const { data: circle, isLoading, refetch } = useCircle(id ?? '');
  const updateCircle = useUpdateCircle();
  const deleteCircle = useDeleteCircle();
  const { data: allAgents } = useAgents();

  const [editingName, setEditingName] = useState(false);
  const [nameDraft, setNameDraft] = useState('');
  const [editingDesc, setEditingDesc] = useState(false);
  const [descDraft, setDescDraft] = useState('');
  const [showDelete, setShowDelete] = useState(false);
  const [showAddMember, setShowAddMember] = useState(false);
  const [showEditParty, setShowEditParty] = useState(false);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- circle manifest channel shape is loosely typed
  const [showEditChannel, setShowEditChannel] = useState<{ index: number; channel: any } | null>(
    null
  );
  const [selectedAgentsForCircle, setSelectedAgentsForCircle] = useState<string[]>([]);
  const [editingContext, setEditingContext] = useState(false);
  const [contextDraft, setContextDraft] = useState('');
  const [savingContext, setSavingContext] = useState(false);

  const agents = circle?.agents ?? [];
  const channels = circle?.channels ?? [];
  const connections = circle?.connections ?? [];
  const partyMode = circle?.partyMode;
  const knowledge = circle?.knowledge;
  const projectContent =
    // eslint-disable-next-line @typescript-eslint/no-explicit-any -- DB circles have `constitution`, YAML have `projectContext`
    (circle as any)?.constitution ??
    (typeof circle?.projectContext === 'object' && circle?.projectContext !== null
      ? // eslint-disable-next-line @typescript-eslint/no-explicit-any
        (circle.projectContext as any).content
      : typeof circle?.projectContext === 'string'
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

  async function handleSaveBasicInfo() {
    if (!id || !circle) return;
    try {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any -- manifest shape varies between DB/YAML circles
      const manifest = { ...(circle as any) };
      if (manifest.metadata) {
        manifest.metadata.displayName = nameDraft.trim() || id;
        manifest.metadata.description = descDraft.trim() || undefined;
      } else {
        manifest.displayName = nameDraft.trim() || id;
        manifest.description = descDraft.trim() || undefined;
      }
      await updateCircle.mutateAsync({ name: id, manifest });
      toast.success('Circle updated');
      setEditingName(false);
      setEditingDesc(false);
      void refetch();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to update');
    }
  }

  async function handleAddMembers() {
    if (!id || !circle || selectedAgentsForCircle.length === 0) return;
    try {
      const currentMembers = circle.agents ?? [];
      const newMembers = [...new Set([...currentMembers, ...selectedAgentsForCircle])];
      await updateCircle.mutateAsync({
        name: id,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        manifest: { ...circle, agents: newMembers } as any,
      });
      toast.success('Members added');
      setShowAddMember(false);
      setSelectedAgentsForCircle([]);
      void refetch();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to add members');
    }
  }

  async function handleRemoveMember(agent: string) {
    if (!id || !circle) return;
    try {
      const newMembers = (circle.agents ?? []).filter((a) => a !== agent);
      await updateCircle.mutateAsync({
        name: id,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        manifest: { ...circle, agents: newMembers } as any,
      });
      toast.success(`Removed ${agent}`);
      void refetch();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to remove member');
    }
  }

  async function handleSavePartyMode(config: CirclePartyModeConfig) {
    if (!id || !circle) return;
    try {
      await updateCircle.mutateAsync({
        name: id,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        manifest: { ...circle, partyMode: config } as any,
      });
      toast.success('Party Mode updated');
      setShowEditParty(false);
      void refetch();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to update Party Mode');
    }
  }

  async function handleSaveChannel(channel: CircleChannelConfig, index?: number) {
    if (!id || !circle) return;
    try {
      const updated = [...(circle.channels ?? [])];
      if (index !== undefined && index >= 0) {
        updated[index] = channel;
      } else {
        updated.push(channel);
      }
      await updateCircle.mutateAsync({
        name: id,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        manifest: { ...circle, channels: updated } as any,
      });
      toast.success(index !== undefined ? 'Channel updated' : 'Channel added');
      setShowEditChannel(null);
      void refetch();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to save channel');
    }
  }

  async function handleDeleteChannel(index: number) {
    if (!id || !circle) return;
    try {
      const updated = (circle.channels ?? []).filter((_, i) => i !== index);
      await updateCircle.mutateAsync({
        name: id,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        manifest: { ...circle, channels: updated } as any,
      });
      toast.success('Channel deleted');
      void refetch();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to delete channel');
    }
  }

  async function handleSaveContext() {
    if (!id) return;
    setSavingContext(true);
    try {
      await circlesApi.updateCircleContext(id, contextDraft);
      toast.success('Project context saved');
      setEditingContext(false);
      void refetch();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to save context');
    } finally {
      setSavingContext(false);
    }
  }

  function startEditName() {
    setNameDraft(circle?.displayName ?? circle?.metadata?.displayName ?? '');
    setEditingName(true);
  }

  function startEditDesc() {
    setDescDraft(circle?.description ?? circle?.metadata?.description ?? '');
    setEditingDesc(true);
  }

  function startEditContext(content?: string) {
    setContextDraft(content ?? '');
    setEditingContext(true);
  }

  return {
    // Data
    circle,
    isLoading,
    allAgents: allAgents ?? [],
    agents,
    channels,
    connections,
    partyMode,
    knowledge,
    projectContent,

    // Name/desc editing
    editingName,
    setEditingName,
    nameDraft,
    setNameDraft,
    startEditName,
    editingDesc,
    setEditingDesc,
    descDraft,
    setDescDraft,
    startEditDesc,

    // Modal state
    showDelete,
    setShowDelete,
    showAddMember,
    setShowAddMember,
    showEditParty,
    setShowEditParty,
    showEditChannel,
    setShowEditChannel,
    selectedAgentsForCircle,
    setSelectedAgentsForCircle,

    // Context editing
    editingContext,
    setEditingContext,
    contextDraft,
    setContextDraft,
    savingContext,
    startEditContext,

    // Mutation state
    isPending: updateCircle.isPending,
    isDeletePending: deleteCircle.isPending,

    // Handlers
    handleDelete,
    handleSaveBasicInfo,
    handleAddMembers,
    handleRemoveMember,
    handleSavePartyMode,
    handleSaveChannel,
    handleDeleteChannel,
    handleSaveContext,
  } as const;
}
