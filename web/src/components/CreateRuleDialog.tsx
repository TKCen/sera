import { useState } from 'react';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogClose,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { useCreateRoutingRule } from '@/hooks/useNotifications';
import type { NotificationChannel } from '@/lib/api/notifications';

const SEVERITY_OPTIONS = ['info', 'warning', 'critical'] as const;

export function CreateRuleDialog({
  open,
  channels,
  onClose,
}: {
  open: boolean;
  channels: NotificationChannel[];
  onClose: () => void;
}) {
  const [eventType, setEventType] = useState('*');
  const [minSeverity, setMinSeverity] = useState('info');
  const [selectedChannels, setSelectedChannels] = useState<string[]>([]);
  const create = useCreateRoutingRule();

  function toggle(id: string) {
    setSelectedChannels((prev) =>
      prev.includes(id) ? prev.filter((c) => c !== id) : [...prev, id]
    );
  }

  function submit() {
    if (!eventType.trim() || selectedChannels.length === 0) return;
    create.mutate(
      { eventType: eventType.trim(), channelIds: selectedChannels, minSeverity },
      {
        onSuccess: () => {
          onClose();
          setEventType('*');
          setSelectedChannels([]);
        },
      }
    );
  }

  return (
    <Dialog open={open} onOpenChange={(o: boolean) => !o && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Add Routing Rule</DialogTitle>
        </DialogHeader>

        <div className="space-y-3">
          <div>
            <label className="sera-label">Event Type Pattern</label>
            <Input
              value={eventType}
              onChange={(e) => setEventType(e.target.value)}
              placeholder="permission.* or * or agent.crashed"
            />
            <p className="text-[11px] text-sera-text-dim mt-1">
              Supports * wildcard, e.g. permission.*
            </p>
          </div>

          <div>
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
            disabled={create.isPending || !eventType.trim() || selectedChannels.length === 0}
          >
            {create.isPending ? 'Creating…' : 'Create'}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
