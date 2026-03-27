import { useState, useCallback } from 'react';
import * as Recharts from 'recharts';
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const RC = Recharts as any;
const {
  AreaChart,
  Area,
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
} = RC;
import { RefreshCw, Download, TrendingUp, Bot, DollarSign, Activity } from 'lucide-react';
import { useUsage } from '@/hooks/useUsage';
import { Skeleton } from '@/components/ui/skeleton';
import { ErrorBoundary } from '@/components/ErrorBoundary';

type Range = '24h' | '7d' | '30d' | 'custom';

function rangeToFromTo(
  range: Range,
  customFrom?: string,
  customTo?: string
): { from: string; to: string } {
  const now = new Date();
  const to = now.toISOString();
  if (range === 'custom') {
    return { from: customFrom ?? to, to: customTo ?? to };
  }
  const ms = range === '24h' ? 86400_000 : range === '7d' ? 7 * 86400_000 : 30 * 86400_000;
  return { from: new Date(now.getTime() - ms).toISOString(), to };
}

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function fmtTs(iso: string, range: Range): string {
  const d = new Date(iso);
  if (range === '24h') return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  return d.toLocaleDateString([], { month: 'short', day: 'numeric' });
}

function SortIcon({ active, dir }: { active: boolean; dir: 'asc' | 'desc' }) {
  if (!active) return <span className="text-sera-text-dim/40">↕</span>;
  return <span className="text-sera-accent">{dir === 'asc' ? '↑' : '↓'}</span>;
}

type SortKey = 'agentName' | 'promptTokens' | 'completionTokens' | 'totalTokens' | 'pctOfTotal';

function InsightsPageContent() {
  const [range, setRange] = useState<Range>('7d');
  const [customFrom, setCustomFrom] = useState('');
  const [customTo, setCustomTo] = useState('');
  const [sortKey, setSortKey] = useState<SortKey>('totalTokens');
  const [sortDir, setSortDir] = useState<'asc' | 'desc'>('desc');

  const { from, to } = rangeToFromTo(range, customFrom, customTo);
  const { data, isLoading, refetch, isFetching } = useUsage({ groupBy: 'agent', from, to });

  const toggleSort = useCallback(
    (key: SortKey) => {
      if (sortKey === key) {
        setSortDir((d) => (d === 'asc' ? 'desc' : 'asc'));
      } else {
        setSortKey(key);
        setSortDir('desc');
      }
    },
    [sortKey]
  );

  const handleDownload = useCallback(async () => {
    if (!data) return;
    const rows = [
      ['Agent', 'Prompt Tokens', 'Completion Tokens', 'Total Tokens', '% of Total'],
      ...(data.byAgent ?? []).map((a) => [
        a.agentName,
        a.promptTokens,
        a.completionTokens,
        a.totalTokens,
        a.pctOfTotal.toFixed(1),
      ]),
    ];
    const csv = rows.map((r) => r.join(',')).join('\n');
    const blob = new Blob([csv], { type: 'text/csv' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `usage-${range}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  }, [data, range]);

  const sortedAgents = [...(data?.byAgent ?? [])].sort((a, b) => {
    const av = a[sortKey] as number | string;
    const bv = b[sortKey] as number | string;
    if (typeof av === 'string')
      return sortDir === 'asc' ? av.localeCompare(bv as string) : (bv as string).localeCompare(av);
    return sortDir === 'asc' ? (av as number) - (bv as number) : (bv as number) - (av as number);
  });

  const chartData = (data?.timeSeries ?? []).map((pt) => ({
    ...pt,
    label: fmtTs(pt.timestamp, range),
  }));

  const ranges: { id: Range; label: string }[] = [
    { id: '24h', label: 'Today' },
    { id: '7d', label: '7 Days' },
    { id: '30d', label: '30 Days' },
    { id: 'custom', label: 'Custom' },
  ];

  return (
    <main aria-label="Insights" className="p-8 max-w-6xl mx-auto space-y-8">
      <header className="sera-page-header">
        <div>
          <h1 className="sera-page-title">Insights</h1>
          <p className="text-sm text-sera-text-muted mt-1">
            LLM token usage and cost across all agents
          </p>
        </div>
        <div className="flex items-center gap-2">
          <nav aria-label="Time range filters" className="flex items-center gap-1 border border-sera-border rounded-lg p-1">
            {ranges.map((r) => (
              <button
                key={r.id}
                onClick={() => setRange(r.id)}
                aria-pressed={range === r.id}
                className={`px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${
                  range === r.id
                    ? 'bg-sera-accent-soft text-sera-accent'
                    : 'text-sera-text-muted hover:text-sera-text'
                }`}
              >
                {r.label}
              </button>
            ))}
          </nav>
          <button
            onClick={() => {
              void refetch();
            }}
            className="sera-btn-ghost p-2"
            title="Refresh"
            aria-label="Refresh data"
          >
            <RefreshCw size={14} className={isFetching ? 'animate-spin' : ''} />
          </button>
          <button
            onClick={() => {
              void handleDownload();
            }}
            className="sera-btn-ghost flex items-center gap-1.5 px-3 py-2 text-xs"
            aria-label="Download CSV"
          >
            <Download size={13} /> CSV
          </button>
        </div>
      </header>

      {range === 'custom' && (
        <section aria-label="Custom time range" className="flex items-center gap-3">
          <div className="space-y-1">
            <label htmlFor="custom-from" className="text-[11px] text-sera-text-dim uppercase tracking-wider">From</label>
            <input
              id="custom-from"
              type="datetime-local"
              value={customFrom}
              onChange={(e) => setCustomFrom(e.target.value)}
              className="sera-input text-xs"
            />
          </div>
          <div className="space-y-1">
            <label htmlFor="custom-to" className="text-[11px] text-sera-text-dim uppercase tracking-wider">To</label>
            <input
              id="custom-to"
              type="datetime-local"
              value={customTo}
              onChange={(e) => setCustomTo(e.target.value)}
              className="sera-input text-xs"
            />
          </div>
        </section>
      )}

      {isLoading ? (
        <div role="status" aria-label="Loading insights" className="space-y-8">
          <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
            <Skeleton className="h-[90px] rounded-xl" />
            <Skeleton className="h-[90px] rounded-xl" />
            <Skeleton className="h-[90px] rounded-xl" />
            <Skeleton className="h-[90px] rounded-xl" />
          </div>
          <Skeleton className="h-[300px] rounded-xl" />
          <Skeleton className="h-[300px] rounded-xl" />
          <Skeleton className="h-[200px] rounded-xl" />
        </div>
      ) : (
        <div aria-live="polite">
          {/* Summary cards */}
          <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
            <SummaryCard
              label="Total Tokens Today"
              value={fmtTokens(data?.summary.totalTokensToday ?? 0)}
              icon={<Activity size={16} className="text-sera-accent" />}
            />
            <SummaryCard
              label="Total Tokens (Period)"
              value={fmtTokens(data?.summary.totalTokensMonth ?? 0)}
              icon={<TrendingUp size={16} className="text-sera-success" />}
            />
            <SummaryCard
              label="Most Active Agent"
              value={data?.summary.mostActiveAgent ?? '—'}
              icon={<Bot size={16} className="text-sera-warning" />}
            />
            <SummaryCard
              label="Estimated Cost"
              value={
                data?.summary.estimatedCost !== undefined
                  ? `$${data.summary.estimatedCost.toFixed(4)}`
                  : '—'
              }
              icon={<DollarSign size={16} className="text-sera-info" />}
            />
          </div>

          {/* Time-series area chart */}
          <div className="sera-card-static p-5">
            <h2 className="text-sm font-semibold text-sera-text mb-4">Token Usage Over Time</h2>
            {chartData.length === 0 ? (
              <p className="text-center text-sera-text-dim text-sm py-10">
                No data for selected range.
              </p>
            ) : (
              <ResponsiveContainer width="100%" height={240}>
                <AreaChart data={chartData} margin={{ top: 4, right: 8, left: 0, bottom: 0 }}>
                  <defs>
                    <linearGradient id="gradPrompt" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor="var(--sera-accent)" stopOpacity={0.3} />
                      <stop offset="95%" stopColor="var(--sera-accent)" stopOpacity={0} />
                    </linearGradient>
                    <linearGradient id="gradCompletion" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor="var(--sera-success)" stopOpacity={0.3} />
                      <stop offset="95%" stopColor="var(--sera-success)" stopOpacity={0} />
                    </linearGradient>
                  </defs>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.05)" />
                  <XAxis dataKey="label" tick={{ fontSize: 10, fill: 'var(--sera-text-dim)' }} />
                  <YAxis
                    tickFormatter={(v: number) => fmtTokens(v)}
                    tick={{ fontSize: 10, fill: 'var(--sera-text-dim)' }}
                  />
                  <Tooltip
                    contentStyle={{
                      background: 'var(--sera-surface)',
                      border: '1px solid var(--sera-border)',
                      borderRadius: '8px',
                      fontSize: 12,
                    }}
                    formatter={(value: number, name: string) => [fmtTokens(value), name]}
                  />
                  <Legend wrapperStyle={{ fontSize: 11 }} />
                  <Area
                    type="monotone"
                    dataKey="promptTokens"
                    name="Prompt"
                    stroke="var(--sera-accent)"
                    fill="url(#gradPrompt)"
                    strokeWidth={2}
                  />
                  <Area
                    type="monotone"
                    dataKey="completionTokens"
                    name="Completion"
                    stroke="var(--sera-success)"
                    fill="url(#gradCompletion)"
                    strokeWidth={2}
                  />
                </AreaChart>
              </ResponsiveContainer>
            )}
          </div>

          {/* Per-agent bar chart */}
          {(data?.byAgent ?? []).length > 0 && (
            <div className="sera-card-static p-5">
              <h2 className="text-sm font-semibold text-sera-text mb-4">Per-Agent Breakdown</h2>
              <ResponsiveContainer width="100%" height={220}>
                <BarChart
                  data={data?.byAgent ?? []}
                  margin={{ top: 4, right: 8, left: 0, bottom: 0 }}
                >
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.05)" />
                  <XAxis
                    dataKey="agentName"
                    tick={{ fontSize: 10, fill: 'var(--sera-text-dim)' }}
                  />
                  <YAxis
                    tickFormatter={(v: number) => fmtTokens(v)}
                    tick={{ fontSize: 10, fill: 'var(--sera-text-dim)' }}
                  />
                  <Tooltip
                    contentStyle={{
                      background: 'var(--sera-surface)',
                      border: '1px solid var(--sera-border)',
                      borderRadius: '8px',
                      fontSize: 12,
                    }}
                    formatter={(value: number, name: string) => [fmtTokens(value), name]}
                  />
                  <Legend wrapperStyle={{ fontSize: 11 }} />
                  <Bar
                    dataKey="promptTokens"
                    name="Prompt"
                    fill="var(--sera-accent)"
                    stackId="a"
                    radius={[0, 0, 0, 0]}
                  />
                  <Bar
                    dataKey="completionTokens"
                    name="Completion"
                    fill="var(--sera-success)"
                    stackId="a"
                    radius={[4, 4, 0, 0]}
                  />
                </BarChart>
              </ResponsiveContainer>
            </div>
          )}

          {/* Per-agent table */}
          <div className="sera-card-static overflow-hidden">
            <div className="p-4 border-b border-sera-border">
              <h2 className="text-sm font-semibold text-sera-text">Agent Usage Table</h2>
            </div>
            {sortedAgents.length === 0 ? (
              <p className="text-center text-sera-text-dim text-sm py-10">No usage data.</p>
            ) : (
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-sera-border text-[11px] uppercase tracking-wider text-sera-text-dim">
                      {(
                        [
                          ['agentName', 'Agent'],
                          ['promptTokens', 'Prompt'],
                          ['completionTokens', 'Completion'],
                          ['totalTokens', 'Total'],
                          ['pctOfTotal', '% of Total'],
                        ] as [SortKey, string][]
                      ).map(([key, label]) => (
                        <th
                          key={key}
                          aria-sort={sortKey === key ? (sortDir === 'asc' ? 'ascending' : 'descending') : 'none'}
                          className="text-left py-3 px-4 cursor-pointer hover:text-sera-text transition-colors select-none"
                          onClick={() => toggleSort(key)}
                        >
                          <span className="flex items-center gap-1">
                            {label}
                            <SortIcon active={sortKey === key} dir={sortDir} />
                          </span>
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {sortedAgents.map((a) => (
                      <tr
                        key={a.agentName}
                        className="border-b border-sera-border/50 hover:bg-sera-surface-hover transition-colors"
                      >
                        <td className="py-3 px-4 text-sera-text font-medium">{a.agentName}</td>
                        <td className="py-3 px-4 text-sera-text-muted font-mono">
                          {fmtTokens(a.promptTokens)}
                        </td>
                        <td className="py-3 px-4 text-sera-text-muted font-mono">
                          {fmtTokens(a.completionTokens)}
                        </td>
                        <td className="py-3 px-4 text-sera-text font-mono font-semibold">
                          {fmtTokens(a.totalTokens)}
                        </td>
                        <td className="py-3 px-4">
                          <div className="flex items-center gap-2">
                            <div className="flex-1 h-1.5 bg-sera-surface-hover rounded-full overflow-hidden">
                              <div
                                className="h-full bg-sera-accent rounded-full"
                                style={{ width: `${Math.min(a.pctOfTotal, 100)}%` }}
                              />
                            </div>
                            <span className="text-xs text-sera-text-muted w-10 text-right">
                              {a.pctOfTotal.toFixed(1)}%
                            </span>
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>

          {/* Model breakdown */}
          {(data?.byModel ?? []).length > 0 && (
            <div className="sera-card-static overflow-hidden">
              <div className="p-4 border-b border-sera-border">
                <h2 className="text-sm font-semibold text-sera-text">Model Breakdown</h2>
              </div>
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-sera-border text-[11px] uppercase tracking-wider text-sera-text-dim">
                      <th className="text-left py-3 px-4">Model</th>
                      <th className="text-right py-3 px-4">Prompt</th>
                      <th className="text-right py-3 px-4">Completion</th>
                      <th className="text-right py-3 px-4">Total</th>
                    </tr>
                  </thead>
                  <tbody>
                    {(data?.byModel ?? []).map((m) => (
                      <tr
                        key={m.model}
                        className="border-b border-sera-border/50 hover:bg-sera-surface-hover transition-colors"
                      >
                        <td className="py-3 px-4 text-sera-text font-mono text-xs">{m.model}</td>
                        <td className="py-3 px-4 text-right text-sera-text-muted font-mono text-xs">
                          {fmtTokens(m.promptTokens)}
                        </td>
                        <td className="py-3 px-4 text-right text-sera-text-muted font-mono text-xs">
                          {fmtTokens(m.completionTokens)}
                        </td>
                        <td className="py-3 px-4 text-right text-sera-text font-mono text-xs font-semibold">
                          {fmtTokens(m.totalTokens)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}
        </div>
      )}
    </main>
  );
}

export default function InsightsPage() {
  return (
    <ErrorBoundary fallbackMessage="The insights page encountered an error.">
      <InsightsPageContent />
    </ErrorBoundary>
  );
}

function SummaryCard({
  label,
  value,
  icon,
}: {
  label: string;
  value: string;
  icon: React.ReactNode;
}) {
  return (
    <div className="sera-card-static p-4">
      <div className="flex items-center justify-between mb-2">
        <span className="text-[11px] text-sera-text-dim uppercase tracking-wider">{label}</span>
        {icon}
      </div>
      <p className="text-xl font-semibold text-sera-text">{value}</p>
    </div>
  );
}
