import { useState, useMemo, useEffect } from 'react';
import { Bot } from 'lucide-react';
import { useAgentSessions } from '@/hooks/useAgents';
import { CommandLogTimeline } from '@/components/CommandLogTimeline';
import { TabLoading } from '@/components/AgentDetailTabLoading';

export function AgentDetailCommandsTab({ id }: { id: string }) {
  const { data: sessions, isLoading } = useAgentSessions(id);
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);

  const sortedSessions = useMemo(
    () =>
      [...(sessions ?? [])].sort(
        (a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime()
      ),
    [sessions]
  );

  // Auto-select most recent session
  useEffect(() => {
    if (!selectedSessionId && sortedSessions.length > 0) {
      setSelectedSessionId(sortedSessions[0]!.id);
    }
  }, [sortedSessions, selectedSessionId]);

  if (isLoading) return <TabLoading />;

  return (
    <div className="flex flex-col h-full overflow-hidden">
      <div className="flex-shrink-0 border-b border-sera-border bg-sera-surface-bright/20 p-3">
        <div className="flex items-center gap-3">
          <label className="text-[10px] font-bold text-sera-text-muted uppercase tracking-wider flex items-center gap-1.5">
            Session Context:
          </label>
          <select
            value={selectedSessionId ?? ''}
            onChange={(e) => setSelectedSessionId(e.target.value)}
            className="text-xs bg-sera-surface border border-sera-border rounded-md px-3 py-1.5 min-w-[240px] font-medium outline-none focus:ring-1 focus:ring-sera-accent"
          >
            {sortedSessions.map((s) => (
              <option key={s.id} value={s.id}>
                {s.title || 'Untitled Session'} — {new Date(s.updatedAt).toLocaleDateString()}
              </option>
            ))}
            {sortedSessions.length === 0 && <option value="">No sessions found</option>}
          </select>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto">
        {selectedSessionId ? (
          <CommandLogTimeline agentId={id} sessionId={selectedSessionId} />
        ) : (
          <div className="flex flex-col items-center justify-center p-12 text-sera-text-muted opacity-50">
            <Bot size={40} className="mb-4 text-sera-accent-soft" />
            <p className="text-sm font-medium">Select a session to view its command log</p>
          </div>
        )}
      </div>
    </div>
  );
}
