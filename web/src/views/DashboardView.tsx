import { useQuery } from '@tanstack/react-query';
import { CheckCircle2, AlertTriangle, User } from 'lucide-react';
import { getHealth, getReadiness, type Readiness, type AuthMe } from '@/lib/api';
import { useAuth } from '@/contexts/auth-context';
import { cn } from '@/lib/utils';
import type { ReactNode } from 'react';

export function DashboardView() {
  const { identity } = useAuth();

  const health = useQuery({
    queryKey: ['health'],
    queryFn: getHealth,
    refetchInterval: 10_000,
  });

  const readiness = useQuery({
    queryKey: ['readiness'],
    queryFn: getReadiness,
    refetchInterval: 10_000,
  });

  return (
    <div className="space-y-6 p-6">
      <div>
        <h1 className="text-2xl font-semibold">Dashboard</h1>
        <p className="text-sm text-zinc-500">Live system status from the rust gateway.</p>
      </div>

      <div className="grid gap-4 md:grid-cols-3">
        <StatusCard
          title="Liveness"
          state={health.isLoading ? 'loading' : health.error ? 'bad' : 'good'}
          detail={
            health.isLoading
              ? '…'
              : health.error
                ? (health.error as Error).message
                : (health.data?.status ?? 'unknown')
          }
        />
        <StatusCard
          title="Readiness"
          state={readinessState(readiness.isLoading, readiness.error, readiness.data)}
          detail={readinessDetail(readiness.isLoading, readiness.error, readiness.data)}
        />
        <IdentityCard identity={identity} />
      </div>
    </div>
  );
}

type CardState = 'good' | 'warn' | 'bad' | 'loading';

function StatusCard({
  title,
  state,
  detail,
}: {
  title: string;
  state: CardState;
  detail: string;
}) {
  const Icon = state === 'good' ? CheckCircle2 : AlertTriangle;
  return (
    <Card>
      <div className="flex items-center justify-between">
        <span className="text-xs uppercase tracking-wider text-zinc-500">{title}</span>
        <Icon
          className={cn(
            'h-4 w-4',
            state === 'good' && 'text-emerald-400',
            state === 'warn' && 'text-amber-400',
            state === 'bad' && 'text-red-400',
            state === 'loading' && 'text-zinc-600',
          )}
        />
      </div>
      <div className="mt-2 text-lg font-medium">{detail}</div>
    </Card>
  );
}

function IdentityCard({ identity }: { identity: AuthMe | null }) {
  return (
    <Card>
      <div className="flex items-center justify-between">
        <span className="text-xs uppercase tracking-wider text-zinc-500">Identity</span>
        <User className="h-4 w-4 text-zinc-400" />
      </div>
      <div className="mt-2 text-lg font-medium">{identity?.sub ?? '—'}</div>
      <div className="text-xs text-zinc-500">
        {identity ? `mode: ${identity.mode} · roles: ${identity.roles.join(', ')}` : ''}
      </div>
    </Card>
  );
}

function Card({ children }: { children: ReactNode }) {
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900 p-4">{children}</div>
  );
}

function readinessState(
  loading: boolean,
  error: unknown,
  data: Readiness | undefined,
): CardState {
  if (loading) return 'loading';
  if (error) return 'bad';
  if (data?.status === 'ready') return 'good';
  return 'warn';
}

function readinessDetail(
  loading: boolean,
  error: unknown,
  data: Readiness | undefined,
): string {
  if (loading) return '…';
  if (error) return (error as Error).message;
  if (!data) return 'unknown';
  return data.runtime_connected ? data.status : `${data.status} (no runtime)`;
}
