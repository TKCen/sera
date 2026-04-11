import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';

import type { CircleChannelConfig } from '@/lib/api/types';

interface EditChannelDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  channelData: {
    index: number;
    channel: CircleChannelConfig;
  } | null;
  onChannelDataChange: (data: { index: number; channel: CircleChannelConfig } | null) => void;
  onSave: (channel: CircleChannelConfig, index?: number) => void;
  isLoading?: boolean;
}

export function EditChannelDialog({
  open,
  channelData,
  onChannelDataChange,
  onSave,
  isLoading,
}: EditChannelDialogProps) {
  const handleNameChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (channelData) {
      onChannelDataChange({
        index: channelData.index,
        channel: { ...channelData.channel, name: e.target.value },
      });
    }
  };

  const handleDescriptionChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (channelData) {
      onChannelDataChange({
        index: channelData.index,
        channel: { ...channelData.channel, description: e.target.value },
      });
    }
  };

  const handleTypeChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    if (channelData) {
      onChannelDataChange({
        index: channelData.index,
        channel: { ...channelData.channel, type: e.target.value as CircleChannelConfig['type'] },
      });
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onChannelDataChange(null)}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{channelData?.index === -1 ? 'Add Channel' : 'Edit Channel'}</DialogTitle>
        </DialogHeader>
        <div className="space-y-3 py-4">
          <div className="space-y-1.5">
            <label className="text-xs text-sera-text-muted">Channel Name</label>
            <Input
              value={channelData?.channel?.name ?? ''}
              onChange={handleNameChange}
              placeholder="e.g. general"
            />
          </div>
          <div className="space-y-1.5">
            <label className="text-xs text-sera-text-muted">Description</label>
            <Input
              value={channelData?.channel?.description ?? ''}
              onChange={handleDescriptionChange}
              placeholder="Channel purpose..."
            />
          </div>
          <div className="space-y-1.5">
            <label className="text-xs text-sera-text-muted">Type</label>
            <select
              value={channelData?.channel?.type ?? 'persistent'}
              onChange={handleTypeChange}
              className="sera-input text-xs w-full"
            >
              <option value="persistent">Persistent</option>
              <option value="ephemeral">Ephemeral</option>
            </select>
          </div>
        </div>
        <DialogFooter>
          <Button variant="ghost" size="sm" onClick={() => onChannelDataChange(null)}>
            Cancel
          </Button>
          <Button
            size="sm"
            onClick={() => {
              if (!channelData) return;
              onSave(channelData.channel, channelData.index === -1 ? undefined : channelData.index);
            }}
            disabled={isLoading}
          >
            Save Channel
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
