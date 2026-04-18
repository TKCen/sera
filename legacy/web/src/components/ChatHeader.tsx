import { useState } from 'react';
import { Brain, Pencil, Trash2, PanelLeftClose, PanelLeftOpen } from 'lucide-react';
import { cn } from '@/lib/utils';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';

interface ChatHeaderProps {
  sessionId: string | null;
  sessions: Array<{ id: string; title: string }>;
  showThinking: boolean;
  onToggleThinking: (value: boolean) => void;
  onRenameSession: (id: string, title: string) => void;
  onDeleteSession: (id: string, e: React.MouseEvent) => void;
  sidebarOpen: boolean;
  onToggleSidebar: (open: boolean) => void;
}

export function ChatHeader({
  sessionId,
  sessions,
  showThinking,
  onToggleThinking,
  onRenameSession,
  onDeleteSession,
  sidebarOpen,
  onToggleSidebar,
}: ChatHeaderProps) {
  const [renamingSession, setRenamingSession] = useState(false);
  const [renameValue, setRenameValue] = useState('');
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);

  const sidebarToggle = (
    <button
      onClick={() => onToggleSidebar(!sidebarOpen)}
      className="p-1.5 rounded hover:bg-sera-surface text-sera-text-muted hover:text-sera-text transition-colors"
      title={sidebarOpen ? 'Close sidebar' : 'Open sidebar'}
      aria-label="Toggle sidebar"
      aria-expanded={sidebarOpen}
    >
      {sidebarOpen ? <PanelLeftClose size={16} /> : <PanelLeftOpen size={16} />}
    </button>
  );

  return (
    <>
      <div className="flex items-center justify-between px-4 py-2 border-b border-sera-border flex-shrink-0">
        <div className="flex items-center gap-2 flex-1 min-w-0">
          {sidebarToggle}
          {sessionId && (
            <div className="flex items-center gap-2 overflow-hidden">
              <span className="text-xs text-sera-text-muted font-mono truncate">
                {sessions.find((s) => s.id === sessionId)?.title ?? 'New Chat'}
              </span>
              <Tooltip content="Rename Session">
                <button
                  onClick={() => {
                    setRenameValue(sessions.find((s) => s.id === sessionId)?.title ?? '');
                    setRenamingSession(true);
                  }}
                  className="p-1 rounded text-sera-text-dim hover:text-sera-accent transition-colors"
                >
                  <Pencil size={12} />
                </button>
              </Tooltip>
              <Tooltip content="Delete Session">
                <button
                  onClick={() => setShowDeleteConfirm(true)}
                  className="p-1 rounded text-sera-text-dim hover:text-sera-error transition-colors"
                >
                  <Trash2 size={12} />
                </button>
              </Tooltip>
            </div>
          )}
        </div>
        <button
          onClick={() => onToggleThinking(!showThinking)}
          className={cn(
            'flex items-center gap-1.5 px-2 py-1 rounded text-[10px] font-medium transition-all border',
            showThinking
              ? 'bg-sera-accent/10 text-sera-accent border-sera-accent/20'
              : 'bg-sera-surface text-sera-text-muted border-sera-border hover:text-sera-text'
          )}
        >
          <Brain size={12} className={showThinking ? 'animate-pulse' : ''} />
          THINKING: {showThinking ? 'ON' : 'OFF'}
        </button>
      </div>

      {/* Rename Dialog */}
      <Dialog open={renamingSession} onOpenChange={setRenamingSession}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Rename Session</DialogTitle>
          </DialogHeader>
          <div className="py-4">
            <Input
              value={renameValue}
              onChange={(e) => setRenameValue(e.target.value)}
              placeholder="Enter session title..."
              autoFocus
            />
          </div>
          <DialogFooter>
            <Button variant="ghost" size="sm" onClick={() => setRenamingSession(false)}>
              Cancel
            </Button>
            <Button
              size="sm"
              onClick={() => {
                if (sessionId) onRenameSession(sessionId, renameValue);
                setRenamingSession(false);
              }}
            >
              Rename
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Dialog */}
      <Dialog open={showDeleteConfirm} onOpenChange={setShowDeleteConfirm}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Session</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-sera-text-muted py-4">
            Are you sure you want to delete this session? This action cannot be undone.
          </p>
          <DialogFooter>
            <Button variant="ghost" size="sm" onClick={() => setShowDeleteConfirm(false)}>
              Cancel
            </Button>
            <Button
              variant="danger"
              size="sm"
              onClick={(e) => {
                // eslint-disable-next-line @typescript-eslint/no-explicit-any
                if (sessionId) onDeleteSession(sessionId, e as any);
                setShowDeleteConfirm(false);
              }}
            >
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
