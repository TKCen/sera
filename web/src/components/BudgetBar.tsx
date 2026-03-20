import { cn } from '@/lib/utils';

interface BudgetBarProps {
  label: string;
  current: number;
  limit?: number;
  className?: string;
}

function utilPct(current: number, limit?: number): number {
  if (!limit || limit <= 0) return 0;
  return Math.min((current / limit) * 100, 100);
}

export function budgetBarColor(pct: number): string {
  if (pct >= 90) return 'bg-sera-error';
  if (pct >= 70) return 'bg-sera-warning';
  return 'bg-sera-success';
}

export function BudgetBar({ label, current, limit, className }: BudgetBarProps) {
  const pct = utilPct(current, limit);
  const color = budgetBarColor(pct);

  return (
    <div className={cn('space-y-1.5', className)}>
      <div className="flex items-center justify-between text-xs">
        <span className="text-sera-text-muted">{label}</span>
        <span className={cn('font-mono font-medium', pct >= 90 ? 'text-sera-error' : pct >= 70 ? 'text-sera-warning' : 'text-sera-text-muted')}>
          {current.toLocaleString()} / {limit !== undefined ? limit.toLocaleString() : '∞'}
        </span>
      </div>
      <div className="h-2 w-full bg-sera-surface-hover rounded-full overflow-hidden" data-testid="budget-bar-track">
        <div
          className={cn('h-full rounded-full transition-all duration-300', color)}
          style={{ width: `${pct}%` }}
          data-testid="budget-bar-fill"
          data-pct={pct}
        />
      </div>
      {limit !== undefined && pct >= 70 && (
        <p className={cn('text-[11px]', pct >= 90 ? 'text-sera-error' : 'text-sera-warning')}>
          {pct >= 90 ? 'Budget exceeded' : 'Approaching limit'} — {pct.toFixed(0)}% used
        </p>
      )}
    </div>
  );
}
