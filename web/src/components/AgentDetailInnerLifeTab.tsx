import { useState } from 'react';
import { Brain, ChevronDown, ChevronRight, Clock, Zap } from 'lucide-react';
import { useScheduleRuns } from '@/hooks/useSchedules';
import { Badge } from '@/components/ui/badge';
import { TabLoading } from '@/components/AgentDetailTabLoading';
import type { ScheduleRun } from '@/lib/api/types';

const CATEGORY_COLORS: Record<string, string> = {
  reflection: 'bg-purple-500/20 text-purple-300 border-purple-500/30',
  knowledge_consolidation: 'bg-blue-500/20 text-blue-300 border-blue-500/30',
  curiosity_research: 'bg-emerald-500/20 text-emerald-300 border-emerald-500/30',
  goal_review: 'bg-amber-500/20 text-amber-300 border-amber-500/30',
  schedule_review: 'bg-rose-500/20 text-rose-300 border-rose-500/30',
};

const CATEGORY_LABELS: Record<string, string> = {
  reflection: 'Reflection',
  knowledge_consolidation: 'Knowledge',
  curiosity_research: 'Curiosity',
  goal_review: 'Goals',
  schedule_review: 'Meta-Review',
};

function CategoryBadge({ category }: { category: string | null }) {
  if (!category) return null;
  const colorClass = CATEGORY_COLORS[category] ?? 'bg-sera-surface text-sera-text-muted';
  const label = CATEGORY_LABELS[category] ?? category;
  return (
    <span
      className={`inline-flex items-center px-2 py-0.5 rounded text-[10px] font-medium border ${colorClass}`}
    >
      {label}
    </span>
  );
}

function StatusBadge({ status }: { status: string }) {
  const variant = status === 'completed' ? 'success' : status === 'failed' ? 'error' : 'default';
  return <Badge variant={variant}>{status}</Badge>;
}

function formatDuration(start: string | null, end: string | null): string {
  if (!start || !end) return '--';
  const ms = new Date(end).getTime() - new Date(start).getTime();
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  return `${(ms / 60_000).toFixed(1)}m`;
}

function formatTokens(usage: ScheduleRun['usage']): string {
  if (!usage) return '--';
  return `${(usage.totalTokens / 1000).toFixed(1)}k tokens`;
}

function RunRow({ run }: { run: ScheduleRun }) {
  const [expanded, setExpanded] = useState(false);

  const resultPreview = run.result
    ? typeof run.result === 'string'
      ? run.result
      : JSON.stringify(run.result)
    : (run.error ?? 'No output');

  return (
    <div className="sera-card-static">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full p-4 flex items-center gap-3 text-left"
      >
        <span className="text-sera-text-dim flex-shrink-0">
          {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        </span>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <span className="text-sm font-medium text-sera-text truncate">{run.scheduleName}</span>
            <CategoryBadge category={run.scheduleCategory} />
            <StatusBadge status={run.status} />
          </div>
          <div className="flex items-center gap-4 text-xs text-sera-text-muted">
            <span className="flex items-center gap-1">
              <Clock size={10} />
              {new Date(run.firedAt ?? run.createdAt).toLocaleString()}
            </span>
            <span className="flex items-center gap-1">
              <Zap size={10} />
              {formatDuration(run.startedAt, run.completedAt)}
            </span>
            <span>{formatTokens(run.usage)}</span>
            {run.exitReason && run.exitReason !== 'success' && (
              <span className="text-sera-error">{run.exitReason}</span>
            )}
          </div>
        </div>
      </button>
      {expanded && (
        <div className="px-4 pb-4 pt-0">
          <pre className="text-xs text-sera-text-muted bg-sera-bg/50 rounded p-3 max-h-64 overflow-auto whitespace-pre-wrap break-words">
            {resultPreview}
          </pre>
        </div>
      )}
    </div>
  );
}

const ALL_CATEGORIES = [
  'reflection',
  'knowledge_consolidation',
  'curiosity_research',
  'goal_review',
  'schedule_review',
] as const;

export function InnerLifeTab({ id }: { id: string }) {
  const [categoryFilter, setCategoryFilter] = useState<string | undefined>(undefined);

  const { data: runs, isLoading } = useScheduleRuns({
    agentId: id,
    category: categoryFilter,
    limit: 50,
  });

  if (isLoading) return <TabLoading />;

  return (
    <div className="p-6 space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Brain size={16} className="text-sera-accent" />
          <h2 className="text-sm font-semibold text-sera-text">
            Inner Life{runs?.length ? ` (${runs.length} runs)` : ''}
          </h2>
        </div>
      </div>

      {/* Category filter */}
      <div className="flex items-center gap-2 flex-wrap">
        <button
          onClick={() => setCategoryFilter(undefined)}
          className={`px-3 py-1 rounded text-xs font-medium transition-colors ${
            !categoryFilter
              ? 'bg-sera-accent/20 text-sera-accent border border-sera-accent/40'
              : 'bg-sera-surface text-sera-text-muted hover:text-sera-text border border-transparent'
          }`}
        >
          All
        </button>
        {ALL_CATEGORIES.map((cat) => (
          <button
            key={cat}
            onClick={() => setCategoryFilter(categoryFilter === cat ? undefined : cat)}
            className={`px-3 py-1 rounded text-xs font-medium transition-colors border ${
              categoryFilter === cat
                ? CATEGORY_COLORS[cat]
                : 'bg-sera-surface text-sera-text-muted hover:text-sera-text border-transparent'
            }`}
          >
            {CATEGORY_LABELS[cat]}
          </button>
        ))}
      </div>

      {/* Runs list */}
      {!runs?.length ? (
        <div className="text-center py-12">
          <Brain size={32} className="mx-auto text-sera-text-dim mb-3" />
          <p className="text-sm text-sera-text-muted">No inner-life activity yet.</p>
          <p className="text-xs text-sera-text-dim mt-1">
            Activate inner-life schedules in the Schedules tab to get started.
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {runs.map((run) => (
            <RunRow key={run.taskId} run={run} />
          ))}
        </div>
      )}
    </div>
  );
}
