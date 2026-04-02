import { memo } from 'react';
import { PanelLeftClose, PanelLeftOpen } from 'lucide-react';
import { ChatSidebar } from '@/components/ChatSidebar';
import type { SessionInfo } from '@/lib/types/chat';
import type { AgentInfo } from '@/hooks/useAgents';

interface SidebarManagerProps {
  sessions: SessionInfo[];
  agents: AgentInfo[] | undefined;
  agentsLoading: boolean;
  agentsError: boolean;
  selectedAgent: string;
  sessionId: string | null;
  sidebarOpen: boolean;
  onSidebarToggle: () => void;
  onAgentChange: (name: string) => void;
  onStartNewSession: () => void;
  onLoadSession: (id: string) => void;
  onDeleteSession: (id: string, e: React.MouseEvent) => void;
  onRenameSession: (id: string, title: string) => void;
  onRefetchAgents: () => void;
}

export const SidebarManager = memo(function SidebarManager({
  sessions,
  agents,
  agentsLoading,
  agentsError,
  selectedAgent,
  sessionId,
  sidebarOpen,
  onSidebarToggle,
  onAgentChange,
  onStartNewSession,
  onLoadSession,
  onDeleteSession,
  onRenameSession,
  onRefetchAgents,
}: SidebarManagerProps) {
  const sidebarToggle = (
    <button
      onClick={onSidebarToggle}
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
      <ChatSidebar
        sessions={sessions}
        agents={agents}
        agentsLoading={agentsLoading}
        agentsError={agentsError}
        selectedAgent={selectedAgent}
        sessionId={sessionId}
        sidebarOpen={sidebarOpen}
        onAgentChange={onAgentChange}
        onStartNewSession={onStartNewSession}
        onLoadSession={onLoadSession}
        onDeleteSession={onDeleteSession}
        onRenameSession={onRenameSession}
        onRefetchAgents={onRefetchAgents}
      />
      {!sidebarOpen && (
        <div className="absolute top-4 left-4 z-10">
          {sidebarToggle}
        </div>
      )}
      {sidebarOpen && (
        <div className="hidden">
           {/* This is a hack to allow the parent to get the toggle button if needed,
               but in ChatPage we handle it differently for the top bar. */}
        </div>
      )}
    </>
  );
});
