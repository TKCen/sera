import { useState } from 'react';
import { Loader2, ChevronDown, ChevronRight, AlertCircle, CheckCircle2, Clock } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useAgentContextDebug } from '@/hooks/useAgents';
import type { ContextDebugResponse } from '@/lib/api/agents';

type ContextAssemblyEvent = ContextDebugResponse['events'][number];

function stageIcon(stage: string) {
  if (stage.includes('error') || stage.includes('skip')) {
    return <AlertCircle size={14} className="text-sera-warning shrink-0" />;
  }
  if (stage.includes('complete') || stage.includes('done')) {
    return <CheckCircle2 size={14} className="text-sera-success shrink-0" />;
  }
  return <Clock size={14} className="text-sera-text-muted shrink-0" />;
}

function stageLabel(stage: string): string {
  return stage
    .replace('context.', '')
    .replace(/_/g, ' ')
    .replace(/\b\w/g, (c) => c.toUpperCase());
}

function TokenBar({ events }: { events: ContextAssemblyEvent[] }) {
  const budgetEvent = events.find((e) => e.stage === 'context.token_budget');
  if (!budgetEvent) return null;
  const detail = budgetEvent.detail as Record<string, number>;
  const total = detail['totalBudget'] ?? detail['contextWindow'] ?? 0;
  if (!total) return null;

  const segments: { label: string; tokens: number; color: string }[] = [
    {
      label: 'System',
      tokens: (detail['systemPromptTokens'] as number) ?? 0,
      color: 'bg-sera-accent',
    },
    { label: 'Memory', tokens: (detail['memoryTokens'] as number) ?? 0, color: 'bg-purple-500' },
    { label: 'Skills', tokens: (detail['skillTokens'] as number) ?? 0, color: 'bg-emerald-500' },
    { label: 'History', tokens: (detail['historyTokens'] as number) ?? 0, color: 'bg-sky-500' },
  ];

  const used = segments.reduce((sum, s) => sum + s.tokens, 0);

  return (
    <div className="rounded-lg border border-sera-border bg-sera-surface p-4">
      <h3 className="text-sm font-medium text-sera-text mb-3">
        Token Budget — {used.toLocaleString()} / {total.toLocaleString()}
      </h3>
      <div className="h-3 rounded-full bg-sera-bg overflow-hidden flex">
        {segments
          .filter((s) => s.tokens > 0)
          .map((s) => (
            <div
              key={s.label}
              className={`${s.color} h-full`}
              style={{ width: `${(s.tokens / total) * 100}%` }}
              title={`${s.label}: ${s.tokens.toLocaleString()} tokens`}
            />
          ))}
      </div>
      <div className="flex gap-4 mt-2">
        {segments
          .filter((s) => s.tokens > 0)
          .map((s) => (
            <div key={s.label} className="flex items-center gap-1.5 text-xs text-sera-text-muted">
              <div className={`w-2 h-2 rounded-full ${s.color}`} />
              {s.label}: {s.tokens.toLocaleString()}
            </div>
          ))}
      </div>
    </div>
  );
}

function EventCard({ event, defaultOpen }: { event: ContextAssemblyEvent; defaultOpen?: boolean }) {
  const [open, setOpen] = useState(defaultOpen ?? false);
  const detailKeys = Object.keys(event.detail);

  return (
    <div className="rounded-lg border border-sera-border bg-sera-surface">
      <button
        onClick={() => setOpen(!open)}
        className="w-full flex items-center gap-2 px-4 py-2.5 text-left hover:bg-sera-bg/50 transition-colors"
      >
        {open ? (
          <ChevronDown size={14} className="text-sera-text-muted shrink-0" />
        ) : (
          <ChevronRight size={14} className="text-sera-text-muted shrink-0" />
        )}
        {stageIcon(event.stage)}
        <span className="text-sm font-medium text-sera-text">{stageLabel(event.stage)}</span>
        {event.durationMs != null && (
          <span className="ml-auto text-xs text-sera-text-muted">{event.durationMs}ms</span>
        )}
      </button>
      {open && detailKeys.length > 0 && (
        <div className="px-4 pb-3 border-t border-sera-border">
          <pre className="text-xs text-sera-text-muted mt-2 overflow-x-auto whitespace-pre-wrap">
            {JSON.stringify(event.detail, null, 2)}
          </pre>
        </div>
      )}
    </div>
  );
}

export function AgentDetailContextTab({ id }: { id: string }) {
  const [testMessage, setTestMessage] = useState('Hello');
  const [queryMessage, setQueryMessage] = useState('Hello');

  const { data, isLoading, isError, error, refetch } = useAgentContextDebug(id, queryMessage);

  const handleRun = () => {
    setQueryMessage(testMessage);
    void refetch();
  };

  return (
    <div className="p-6 space-y-4">
      <div className="flex items-center gap-2">
        <h2 className="text-lg font-semibold text-sera-text">Context Debug</h2>
        <span className="text-xs text-sera-text-muted">
          Dry-run context assembly to inspect what gets sent to the LLM
        </span>
      </div>

      {/* Test message input */}
      <div className="flex gap-2">
        <Input
          value={testMessage}
          onChange={(e) => setTestMessage(e.target.value)}
          placeholder="Test message to simulate..."
          className="flex-1"
          onKeyDown={(e) => {
            if (e.key === 'Enter') handleRun();
          }}
        />
        <Button onClick={handleRun} size="sm" disabled={isLoading}>
          {isLoading ? <Loader2 size={14} className="animate-spin" /> : 'Run'}
        </Button>
      </div>

      {isError && (
        <div className="rounded-lg border border-sera-error/30 bg-sera-error/5 p-4 text-sm text-sera-error">
          {error instanceof Error ? error.message : 'Failed to load context debug'}
        </div>
      )}

      {data && (
        <>
          {/* Summary header */}
          <div className="flex items-center gap-4 text-xs text-sera-text-muted">
            <span>Agent: {data.agentName}</span>
            <span>
              System prompt: ~{Math.round(data.systemPromptLength / 4).toLocaleString()} tokens
            </span>
            <span>Events: {data.events.length}</span>
          </div>

          {/* Token budget bar */}
          <TokenBar events={data.events} />

          {/* Event pipeline */}
          <div className="space-y-1.5">
            <h3 className="text-sm font-medium text-sera-text mb-2">Assembly Pipeline</h3>
            {data.events.map((event, i) => (
              <EventCard
                key={`${event.stage}-${i}`}
                event={event}
                defaultOpen={event.stage.includes('error')}
              />
            ))}
          </div>
        </>
      )}
    </div>
  );
}
