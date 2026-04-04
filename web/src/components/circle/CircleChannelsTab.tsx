import { Radio, Pencil, Trash2, Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import type { CircleChannelConfig } from '@/lib/api/types';

interface CircleChannelsTabProps {
  channels: CircleChannelConfig[];
  onAddChannel: () => void;
  onEditChannel: (index: number, channel: CircleChannelConfig) => void;
  onDeleteChannel: (index: number) => void;
}

export function CircleChannelsTab({
  channels,
  onAddChannel,
  onEditChannel,
  onDeleteChannel,
}: CircleChannelsTabProps) {
  return (
    <div>
      <div className="flex justify-end mb-4">
        <Button size="sm" onClick={onAddChannel}>
          <Plus size={14} /> Add Channel
        </Button>
      </div>
      {channels.length === 0 ? (
        <p className="text-xs text-sera-text-dim py-8 text-center">
          No channels configured for this circle.
        </p>
      ) : (
        <div className="space-y-2">
          {channels.map((ch, i) => (
            <div
              key={ch.id ?? ch.name ?? i}
              className="sera-card-static rounded-lg px-4 py-3 flex items-center gap-3 group/channel"
            >
              <Radio size={14} className="text-sera-text-muted flex-shrink-0" />
              <div className="flex-1 min-w-0">
                <span className="text-sm font-medium text-sera-text">{ch.name}</span>
                {ch.description && (
                  <p className="text-xs text-sera-text-dim mt-0.5">{ch.description}</p>
                )}
              </div>
              <div className="flex items-center gap-2">
                {ch.type && (
                  <Badge variant={ch.type === 'persistent' ? 'accent' : 'warning'}>{ch.type}</Badge>
                )}
                {ch.id && <span className="text-[10px] text-sera-text-dim font-mono">{ch.id}</span>}
                <button
                  onClick={() => onEditChannel(i, ch)}
                  className="p-1 rounded text-sera-text-dim opacity-0 group-hover/channel:opacity-100 hover:bg-sera-surface-hover transition-all"
                >
                  <Pencil size={12} />
                </button>
                <button
                  onClick={() => void onDeleteChannel(i)}
                  className="p-1 rounded text-sera-text-dim opacity-0 group-hover/channel:opacity-100 hover:bg-sera-error/10 hover:text-sera-error transition-all"
                >
                  <Trash2 size={12} />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
