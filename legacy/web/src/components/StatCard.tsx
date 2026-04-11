import { TrendingUp, TrendingDown } from 'lucide-react';
import { Card } from '@/components/ui/card';
import { cn } from '@/lib/utils';

interface StatCardProps {
  label: string;
  value: string | number;
  trend?: number;
  trendLabel?: string;
  className?: string;
}

export function StatCard({ label, value, trend, trendLabel, className }: StatCardProps) {
  const isPositive = trend !== undefined && trend >= 0;
  const TrendIcon = isPositive ? TrendingUp : TrendingDown;

  return (
    <Card className={cn('p-5', className)}>
      <div className="text-xs text-sera-text-muted uppercase tracking-wider mb-2">{label}</div>
      <div className="text-2xl font-semibold text-sera-text tabular-nums">{value}</div>
      {trend !== undefined && (
        <div
          className={cn(
            'mt-2 flex items-center gap-1 text-xs',
            isPositive ? 'text-sera-success' : 'text-sera-error'
          )}
        >
          <TrendIcon size={12} />
          <span>
            {isPositive ? '+' : ''}
            {trend}%{trendLabel ? ` ${trendLabel}` : ''}
          </span>
        </div>
      )}
    </Card>
  );
}
