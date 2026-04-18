import { Badge } from '@/components/ui/badge';
import type { Schedule } from '@/lib/api/types';

export function statusBadge(status: Schedule['status']) {
  return <Badge variant={status === 'active' ? 'success' : 'default'}>{status}</Badge>;
}

export function lastRunBadge(s?: Schedule['lastRunStatus']) {
  if (!s) return null;
  const variant = s === 'success' ? 'success' : s === 'error' ? 'error' : 'warning';
  return <Badge variant={variant}>{s}</Badge>;
}
