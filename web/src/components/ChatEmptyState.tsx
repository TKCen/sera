import { Bot } from 'lucide-react';

interface ChatEmptyStateProps {
  sidebarToggle: React.ReactNode;
  isAgentUnavailable: boolean;
  agentStatus: string | undefined;
  selectedAgent: string;
  selectedAgentData: Record<string, unknown> | undefined;
  selectedAgentId: string;
  inputBar: React.ReactNode;
}

export function ChatEmptyState({
  sidebarToggle,
  isAgentUnavailable,
  agentStatus,
  selectedAgent,
  selectedAgentData,
  selectedAgentId,
  inputBar,
}: ChatEmptyStateProps) {
  return (
    <div className="flex-1 flex flex-col items-center justify-center px-8 relative">
      <div className="absolute top-4 left-4">{sidebarToggle}</div>
      <div className="w-16 h-16 rounded-2xl bg-sera-accent-soft flex items-center justify-center mb-6">
        <Bot size={32} className="text-sera-accent" />
      </div>
      <h2 className="text-xl font-semibold text-sera-text mb-2">How can I help you?</h2>
      {isAgentUnavailable && (
        <div className="mb-4 px-4 py-2 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm max-w-md text-center">
          Agent is {agentStatus} — messages may not be delivered. Try restarting from the{' '}
          <a href={`/agents/${selectedAgentId}`} className="underline hover:text-red-300">
            agent detail page
          </a>
          .
        </div>
      )}
      <p className="text-sm text-sera-text-muted mb-8 text-center max-w-md">
        {selectedAgent
          ? `Chatting with ${(selectedAgentData?.display_name as string) ?? selectedAgent}`
          : 'Select an agent from the sidebar to get started.'}
      </p>
      <div className="w-full max-w-2xl">{inputBar}</div>
    </div>
  );
}
