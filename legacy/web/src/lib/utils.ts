import { type ClassValue, clsx } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function utilPct(current: number, limit?: number): number {
  if (!limit || limit <= 0) return 0;
  return Math.min((current / limit) * 100, 100);
}

export function budgetBarColor(pct: number): string {
  if (pct >= 90) return 'bg-sera-error';
  if (pct >= 70) return 'bg-sera-warning';
  return 'bg-sera-success';
}

export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}

/** Format an ISO timestamp as HH:MM:SS */
export function formatTime(ts: string): string {
  const d = new Date(ts);
  return isNaN(d.getTime())
    ? ''
    : d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

/** Format elapsed time between two ISO timestamps as human-readable string */
export function formatElapsed(from: string, to: string): string {
  const ms = new Date(to).getTime() - new Date(from).getTime();
  if (isNaN(ms) || ms < 0) return '';
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.floor(ms / 60000)}m ${Math.round((ms % 60000) / 1000)}s`;
}

export function formatDistanceToNow(isoDate: string): string {
  const ms = new Date(isoDate).getTime() - Date.now();
  const abs = Math.abs(ms);
  const past = ms < 0;

  if (abs < 60_000) return past ? 'just now' : 'in a moment';
  if (abs < 3_600_000) {
    const m = Math.round(abs / 60_000);
    return past ? `${m}m ago` : `in ${m}m`;
  }
  if (abs < 86_400_000) {
    const h = Math.round(abs / 3_600_000);
    return past ? `${h}h ago` : `in ${h}h`;
  }
  const d = Math.round(abs / 86_400_000);
  return past ? `${d}d ago` : `in ${d}d`;
}
