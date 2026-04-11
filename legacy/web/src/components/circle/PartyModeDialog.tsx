import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';

import type { CirclePartyModeConfig } from '@/lib/api/types';

interface PartyModeDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  partyMode: CirclePartyModeConfig | undefined;
  agents: string[];
  onSave: (config: CirclePartyModeConfig) => void;
  isLoading?: boolean;
}

export function PartyModeDialog({
  open,
  onOpenChange,
  partyMode,
  agents,
  onSave,
  isLoading,
}: PartyModeDialogProps) {
  if (!partyMode) {
    return null;
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Party Mode Settings</DialogTitle>
        </DialogHeader>
        <div className="space-y-4 py-4">
          <div className="flex items-center justify-between">
            <label className="text-sm font-medium">Enabled</label>
            <input
              type="checkbox"
              checked={partyMode.enabled}
              onChange={(e) => onSave({ ...partyMode, enabled: e.target.checked })}
              className="h-4 w-4 rounded border-sera-border bg-sera-surface text-sera-accent"
            />
          </div>
          <div className="space-y-1.5">
            <label className="text-xs text-sera-text-muted">Orchestrator Agent</label>
            <select
              value={partyMode.orchestrator ?? ''}
              onChange={(e) => onSave({ ...partyMode, orchestrator: e.target.value })}
              className="sera-input text-xs w-full"
            >
              <option value="">None</option>
              {agents.map((a) => (
                <option key={a} value={a}>
                  {a}
                </option>
              ))}
            </select>
          </div>
          <div className="space-y-1.5">
            <label className="text-xs text-sera-text-muted">Selection Strategy</label>
            <select
              value={partyMode.selectionStrategy ?? 'relevance'}
              onChange={(e) =>
                onSave({
                  ...partyMode,
                  selectionStrategy: e.target.value as CirclePartyModeConfig['selectionStrategy'],
                })
              }
              className="sera-input text-xs w-full"
            >
              <option value="relevance">Relevance</option>
              <option value="round-robin">Round Robin</option>
              <option value="all">All</option>
            </select>
          </div>
        </div>
        <DialogFooter>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => onOpenChange(false)}
            disabled={isLoading}
          >
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
