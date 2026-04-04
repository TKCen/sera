import { Bot } from 'lucide-react';
import { useChatPage } from '@/hooks/useChatPage';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { ChatSidebar } from '@/components/ChatSidebar';
import { ChatHeader } from '@/components/ChatHeader';
import { ChatInputBar } from '@/components/ChatInputBar';
import { ChatMessageBubble } from '@/components/ChatMessageBubble';
import { EmptyState } from '@/components/EmptyState';

// ── Types ────────────────────────────────────────────────────────────────────

import type { Message, MessageThought } from '@/lib/api/types';
export type { Message, MessageThought };

// ── ChatPage ──────────────────────────────────────────────────────────────────

function ChatPageContent() {
  const {
    agents,
    agentsLoading,
    agentsError,
    refetchAgents,
    selectedAgent,
    selectedAgentId,
    messages,
    input,
    setInput,
    streaming,
    sessions,
    sessionId,
    sidebarOpen,
    setSidebarOpen,
    showThinking,
    setShowThinking,
    expandedThoughts,
    inputRef,
    messagesEndRef,
    loadSession,
    startNewSession,
    deleteSession,
    renameSession,
    toggleThoughts,
    handleSend,
    handleKeyDown,
    handleAgentChange,
    handleCancel,
    queueCount,
  } = useChatPage();

  // ── Selected agent status ────────────────────────────────────────────────────
  const selectedAgentData = agents?.find((a) => a.name === selectedAgent);
  const agentStatus = (selectedAgentData as Record<string, unknown> | undefined)?.status as
    | string
    | undefined;
  const isAgentUnavailable = agentStatus === 'error' || agentStatus === 'stopped';

  // ── Empty state ───────────────────────────────────────────────────────────────

  if (messages.length === 0 && !streaming) {
    return (
      <main className="flex h-full">
        <ChatSidebar
          sessions={sessions}
          agents={agents}
          agentsLoading={agentsLoading}
          agentsError={agentsError}
          selectedAgent={selectedAgent}
          sessionId={sessionId}
          sidebarOpen={sidebarOpen}
          onAgentChange={handleAgentChange}
          onStartNewSession={startNewSession}
          onLoadSession={(id) => void loadSession(id)}
          onDeleteSession={(id, e) => void deleteSession(id, e)}
          onRenameSession={(id, title) => void renameSession(id, title)}
          onRefetchAgents={() => void refetchAgents()}
        />
        <div className="flex-1 flex flex-col items-center justify-center px-8 relative">
          <div className="absolute top-4 left-4">
            <button
              onClick={() => setSidebarOpen((v) => !v)}
              className="p-1.5 rounded hover:bg-sera-surface text-sera-text-muted hover:text-sera-text transition-colors"
              title={sidebarOpen ? 'Close sidebar' : 'Open sidebar'}
              aria-label="Toggle sidebar"
              aria-expanded={sidebarOpen}
            >
              {sidebarOpen ? (
                <svg
                  width="16"
                  height="16"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                >
                  <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
                  <line x1="9" y1="3" x2="9" y2="21" />
                </svg>
              ) : (
                <svg
                  width="16"
                  height="16"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                >
                  <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
                  <line x1="15" y1="3" x2="15" y2="21" />
                </svg>
              )}
            </button>
          </div>
          <div className="flex-1 w-full flex flex-col items-center justify-center">
            <EmptyState
              icon={<Bot size={32} className="text-sera-accent" />}
              title="How can I help you?"
              description={
                selectedAgent
                  ? `Chatting with ${selectedAgentData?.display_name ?? selectedAgent}`
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
          <div className="w-full max-w-2xl pb-8">
            <ChatInputBar
              inputRef={inputRef}
              input={input}
              setInput={setInput}
              handleKeyDown={handleKeyDown}
              streaming={streaming}
              selectedAgent={selectedAgent}
              handleSend={() => void handleSend()}
              onCancel={handleCancel}
              queueCount={queueCount}
            />
          </div>
        </div>
      </main>
    );
  }

  // ── Conversation view ─────────────────────────────────────────────────────────

  return (
    <main className="flex h-full">
      <ChatSidebar
        sessions={sessions}
        agents={agents}
        agentsLoading={agentsLoading}
        agentsError={agentsError}
        selectedAgent={selectedAgent}
        sessionId={sessionId}
        sidebarOpen={sidebarOpen}
        onAgentChange={handleAgentChange}
        onStartNewSession={startNewSession}
        onLoadSession={(id) => void loadSession(id)}
        onDeleteSession={(id, e) => void deleteSession(id, e)}
        onRenameSession={(id, title) => void renameSession(id, title)}
        onRefetchAgents={() => void refetchAgents()}
      />

      <div className="flex-1 flex flex-col min-w-0 h-full">
        <ChatHeader
          sessionId={sessionId}
          sessions={sessions}
          showThinking={showThinking}
          onToggleThinking={setShowThinking}
          onRenameSession={renameSession}
          onDeleteSession={deleteSession}
          sidebarOpen={sidebarOpen}
          onToggleSidebar={setSidebarOpen}
        />

        {/* Messages */}
        <div
          className="flex-1 overflow-y-auto px-6 py-6 space-y-5 min-h-0"
          role="log"
          aria-live="polite"
        >
          {messages.map((msg) => (
            <ChatMessageBubble
              key={msg.id}
              msg={msg}
              showThinking={showThinking}
              isExpanded={expandedThoughts.has(msg.id)}
              onToggleThoughts={toggleThoughts}
            />
          ))}
          <div ref={messagesEndRef} />
        </div>

        {/* Input */}
        <div className="px-6 py-4 border-t border-sera-border flex-shrink-0">
          <div className="max-w-3xl mx-auto">
            <ChatInputBar
              inputRef={inputRef}
              input={input}
              setInput={setInput}
              handleKeyDown={handleKeyDown}
              streaming={streaming}
              selectedAgent={selectedAgent}
              handleSend={() => void handleSend()}
              onCancel={handleCancel}
              queueCount={queueCount}
            />
          </div>
        </div>
      </div>
    </main>
  );
}

export default function ChatPage() {
  return (
    <ErrorBoundary fallbackMessage="The chat interface encountered an error.">
      <ChatPageContent />
    </ErrorBoundary>
  );
}
