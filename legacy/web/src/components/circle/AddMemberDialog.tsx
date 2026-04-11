import { Button } from '@/components/ui/button';
import { MultiSelectPicker } from '@/components/MultiSelectPicker';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '@/components/ui/dialog';
import type { AgentInstance } from '@/lib/api/types';

interface AddMemberDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  allAgents: AgentInstance[];
  currentMemberIds: string[];
  selectedAgents: string[];
  onSelectedAgentsChange: (agents: string[]) => void;
  onMembersAdded: () => void;
  isLoading?: boolean;
}

export function AddMemberDialog({
  open,
  onOpenChange,
  allAgents,
  currentMemberIds,
  selectedAgents,
  onSelectedAgentsChange,
  onMembersAdded,
  isLoading,
}: AddMemberDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Add Members</DialogTitle>
          <DialogDescription>Select agents to add to this circle.</DialogDescription>
        </DialogHeader>
        <div className="py-4">
          <MultiSelectPicker
            items={(allAgents ?? [])
              .filter((a) => !currentMemberIds.includes(a.name))
              .map((a) => ({ id: a.name, label: a.display_name ?? a.name }))}
            selected={selectedAgents}
            onChange={onSelectedAgentsChange}
            placeholder="Search agents..."
          />
        </div>
        <DialogFooter>
          <Button variant="ghost" size="sm" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            size="sm"
            onClick={onMembersAdded}
            disabled={selectedAgents.length === 0 || isLoading}
          >
            Add Selected
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
