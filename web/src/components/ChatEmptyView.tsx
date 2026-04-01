import React from 'react';
import { Bot, PanelLeftClose, PanelLeftOpen } from 'lucide-react';
import { EmptyState } from '@/components/EmptyState';

interface ChatEmptyViewProps {
  selectedAgent: string;
  selectedAgentDisplayName?: string;
  selectedAgentId?: string;
  agentStatus?: string;
  sidebarOpen: boolean;
  setSidebarOpen: (open: boolean | ((v: boolean) => boolean)) => void;
  children?: React.ReactNode;
}

export const ChatEmptyView: React.FC<ChatEmptyViewProps> = ({
  selectedAgent,
  selectedAgentDisplayName,
  selectedAgentId,
  agentStatus,
  sidebarOpen,
  setSidebarOpen,
  children,
}) => {
  const isAgentUnavailable = agentStatus === 'error' || agentStatus === 'stopped';

  return (
    <div className="flex-1 flex flex-col items-center justify-center px-8 relative">
      <div className="absolute top-4 left-4">
        <button
          onClick={() => setSidebarOpen((v) => !v)}
          className="p-1.5 rounded hover:bg-sera-surface text-sera-text-muted hover:text-sera-text transition-colors"
          title={sidebarOpen ? 'Close sidebar' : 'Open sidebar'}
          aria-label="Toggle sidebar"
          aria-expanded={sidebarOpen}
        >
          {sidebarOpen ? <PanelLeftClose size={16} /> : <PanelLeftOpen size={16} />}
        </button>
      </div>
      <div className="flex-1 w-full flex flex-col items-center justify-center">
        <EmptyState
          icon={<Bot size={32} className="text-sera-accent" />}
          title="How can I help you?"
          description={
            selectedAgent
              ? `Chatting with ${selectedAgentDisplayName ?? selectedAgent}`
              : 'Select an agent from the sidebar to get started.'
          }
        />
        {isAgentUnavailable && (
          <div className="mb-4 px-4 py-2 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm max-w-md text-center">
            Agent is {agentStatus} — messages may not be delivered. Try restarting from the{' '}
            <a href={`/agents/${selectedAgentId}`} className="underline hover:text-red-300">
              agent detail page
            </a>
            .
          </div>
        )}
      </div>
      {children && <div className="w-full max-w-2xl pb-8">{children}</div>}
    </div>
  );
};
