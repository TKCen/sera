import { useState, useEffect } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogClose,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { useCreateRoutingRule, useUpdateRoutingRule } from '@/hooks/useNotifications';
import { useAgents } from '@/hooks/useAgents';
import type { NotificationChannel, RoutingRule } from '@/lib/api/notifications';

const SEVERITY_OPTIONS = ['info', 'warning', 'critical'] as const;

export function RuleDialog({
  open,
  channels,
  onClose,
  initialData,
}: {
  open: boolean;
  channels: NotificationChannel[];
  onClose: () => void;
  initialData?: RoutingRule;
}) {
  const [eventType, setEventType] = useState('*');
  const [minSeverity, setMinSeverity] = useState('info');
  const [priority, setPriority] = useState(0);
  const [enabled, setEnabled] = useState(true);
  const [targetAgentId, setTargetAgentId] = useState('');
  const [selectedChannels, setSelectedChannels] = useState<string[]>([]);

  const create = useCreateRoutingRule();
  const update = useUpdateRoutingRule();
  const { data: agents } = useAgents();

  useEffect(() => {
    if (initialData) {
      setEventType(initialData.eventType);
      setMinSeverity(initialData.minSeverity);
      setPriority(initialData.priority);
      setEnabled(initialData.enabled);
      setTargetAgentId(initialData.targetAgentId || '');
      setSelectedChannels(initialData.channelIds);
    } else {
      setEventType('*');
      setMinSeverity('info');
      setPriority(0);
      setEnabled(true);
      setTargetAgentId('');
      setSelectedChannels([]);
    }
  }, [initialData, open]);

  function toggle(id: string) {
    setSelectedChannels((prev) =>
      prev.includes(id) ? prev.filter((c) => c !== id) : [...prev, id]
    );
  }

  function submit() {
    if (!eventType.trim() || selectedChannels.length === 0) return;
    const payload = {
      eventType: eventType.trim(),
      channelIds: selectedChannels,
      minSeverity,
      priority,
      enabled,
      targetAgentId: targetAgentId || null,
    };

    if (initialData) {
      update.mutate(
        { id: initialData.id, data: payload },
        {
          onSuccess: () => {
            onClose();
          },
        }
      );
    } else {
      create.mutate(payload, {
        onSuccess: () => {
          onClose();
        },
      });
    }
  }

  const isPending = create.isPending || update.isPending;

  return (
    <Dialog open={open} onOpenChange={(o: boolean) => !o && onClose()}>
      <DialogContent className="max-w-md max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{initialData ? 'Edit Routing Rule' : 'Add Routing Rule'}</DialogTitle>
        </DialogHeader>

        <div className="space-y-3">
          <div className="flex gap-4">
            <div className="flex-1">
              <label className="sera-label">Event Type Pattern</label>
              <Input
                value={eventType}
                onChange={(e) => setEventType(e.target.value)}
                placeholder="permission.* or * or agent.crashed"
              />
            </div>
            <div className="w-24">
              <label className="sera-label">Priority</label>
              <Input
                type="number"
                value={priority}
                onChange={(e) => setPriority(parseInt(e.target.value, 10) || 0)}
              />
            </div>
          </div>
          <p className="text-[11px] text-sera-text-dim">Supports * wildcard, e.g. permission.*</p>

          <div className="flex gap-4">
            <div className="flex-1">
              <label className="sera-label">Minimum Severity</label>
              <select
                className="w-full rounded border border-sera-border bg-sera-surface text-sera-text px-3 py-2 text-sm"
                value={minSeverity}
                onChange={(e) => setMinSeverity(e.target.value)}
              >
                {SEVERITY_OPTIONS.map((s) => (
                  <option key={s} value={s}>
                    {s}
                  </option>
                ))}
              </select>
            </div>
            <div className="flex items-end pb-2">
              <label className="flex items-center gap-2 cursor-pointer text-sm text-sera-text">
                <input
                  type="checkbox"
                  checked={enabled}
                  onChange={(e) => setEnabled(e.target.checked)}
                  className="accent-sera-accent"
                />
                Enabled
              </label>
            </div>
          </div>

          <div>
            <label className="sera-label">Target Agent (optional)</label>
            <select
              value={targetAgentId}
              onChange={(e) => setTargetAgentId(e.target.value)}
              className="sera-input text-sm"
            >
              <option value="">Any Agent</option>
              {agents?.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.display_name ?? a.name} ({a.id.substring(0, 8)})
                </option>
              ))}
            </select>
          </div>

          <div>
            <label className="sera-label">Target Channels</label>
            <div className="space-y-1 max-h-40 overflow-y-auto border border-sera-border rounded p-2">
              {channels.map((ch) => (
                <label
                  key={ch.id}
                  className="flex items-center gap-2 cursor-pointer text-sm text-sera-text"
                >
                  <input
                    type="checkbox"
                    checked={selectedChannels.includes(ch.id)}
                    onChange={() => toggle(ch.id)}
                  />
                  {ch.name}
                  <span className="text-sera-text-dim text-xs">({ch.type})</span>
                </label>
              ))}
              {channels.length === 0 && (
                <p className="text-sera-text-dim text-xs">No channels configured yet.</p>
              )}
            </div>
          </div>
        </div>

        <div className="flex justify-end gap-2 mt-4">
          <DialogClose asChild>
            <Button variant="ghost">Cancel</Button>
          </DialogClose>
          <Button
            onClick={submit}
            disabled={isPending || !eventType.trim() || selectedChannels.length === 0}
          >
            {isPending ? 'Saving…' : initialData ? 'Update' : 'Create'}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
