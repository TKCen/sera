import { Link } from 'react-router';
import {
  Bot,
  Activity,
  MessageSquare,
  Clock,
  Circle,
  AlertTriangle,
  CheckCircle,
  XCircle,
} from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { useAgents } from '@/hooks/useAgents';
import { useHealthDetail } from '@/hooks/useHealth';
import { useCircles } from '@/hooks/useCircles';
import { useSchedules } from '@/hooks/useSchedules';
import { request } from '@/lib/api/client';
import { cn, formatDistanceToNow } from '@/lib/utils';

interface SessionSummary {
  id: string;
  agentName: string;
  title: string;
  messageCount: number;
  updatedAt: string;
}

function useSessions() {
  return useQuery({
    queryKey: ['sessions-recent'],
    queryFn: () => request<SessionSummary[]>('/sessions'),
  });
}

function StatCard({
  label,
  value,
  icon: Icon,
  to,
  accent,
}: {
  label: string;
  value: string | number;
  icon: React.ComponentType<{ size?: number; className?: string }>;
  to: string;
  accent?: string;
}) {
  return (
    <Link
      to={to}
      className="sera-card-static p-4 hover:border-sera-accent/40 transition-colors group"
    >
      <div className="flex items-center justify-between mb-2">
        <Icon
          size={18}
          className={cn(
            'text-sera-text-muted group-hover:text-sera-accent transition-colors',
            accent
          )}
        />
      </div>
      <div className="text-2xl font-bold text-sera-text">{value}</div>
      <div className="text-xs text-sera-text-muted mt-0.5">{label}</div>
    </Link>
  );
}

function HealthBanner({ status }: { status: 'healthy' | 'degraded' | 'unhealthy' | string }) {
  if (status === 'healthy') {
    return (
      <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-sera-success/10 border border-sera-success/20 text-xs text-sera-success">
        <CheckCircle size={14} /> All systems operational
      </div>
    );
  }
  if (status === 'degraded') {
    return (
      <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-yellow-500/10 border border-yellow-500/20 text-xs text-yellow-400">
        <AlertTriangle size={14} /> Some services degraded
      </div>
    );
  }
  return (
    <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-sera-error/10 border border-sera-error/20 text-xs text-sera-error">
      <XCircle size={14} /> System unhealthy
    </div>
  );
}

function RecentSessions() {
  const { data: sessions } = useSessions();
  const recent = (sessions ?? [])
    .sort((a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime())
    .slice(0, 5);

  if (!recent.length) return null;

  return (
    <section className="sera-card-static p-4">
      <div className="flex items-center justify-between mb-3">
        <h2 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider">
          Recent Sessions
        </h2>
        <Link to="/chat" className="text-[11px] text-sera-accent hover:underline">
          View all
        </Link>
      </div>
      <div className="space-y-1.5">
        {recent.map((s) => (
          <Link
            key={s.id}
            to="/chat"
            className="flex items-center gap-3 px-3 py-2 rounded-lg hover:bg-sera-surface-hover transition-colors"
          >
            <MessageSquare size={13} className="text-sera-text-muted flex-shrink-0" />
            <span className="text-sm text-sera-text flex-1 truncate">{s.title}</span>
            <span className="text-[10px] text-sera-text-dim">{s.agentName}</span>
            <span className="text-[10px] text-sera-text-dim">
              {s.messageCount} msg{s.messageCount !== 1 ? 's' : ''}
            </span>
            <span className="text-[10px] text-sera-text-dim">
              {formatDistanceToNow(s.updatedAt)}
            </span>
          </Link>
        ))}
      </div>
    </section>
  );
}

export default function DashboardPage() {
  const { data: agents } = useAgents();
  const { data: health } = useHealthDetail();
  const { data: circles } = useCircles();
  const { data: schedules } = useSchedules({});

  const running = agents?.filter((a) => a.status === 'running').length ?? 0;
  const errored = agents?.filter((a) => a.status === 'error').length ?? 0;
  const totalAgents = agents?.length ?? 0;
  const activeSchedules = schedules?.filter((s) => s.status === 'active').length ?? 0;

  return (
    <div className="p-8 max-w-5xl mx-auto space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="sera-page-title">Dashboard</h1>
          <p className="text-sm text-sera-text-muted mt-1">SERA platform overview</p>
        </div>
        {health && <HealthBanner status={health.status} />}
      </div>

      {/* Stats grid */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
        <StatCard label="Total agents" value={totalAgents} icon={Bot} to="/agents" />
        <StatCard
          label="Running"
          value={running}
          icon={Activity}
          to="/agents"
          accent="text-sera-success"
        />
        <StatCard label="Circles" value={circles?.length ?? 0} icon={Circle} to="/circles" />
        <StatCard label="Active schedules" value={activeSchedules} icon={Clock} to="/schedules" />
      </div>

      {/* Agent status breakdown */}
      {totalAgents > 0 && (
        <section className="sera-card-static p-4">
          <h2 className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider mb-3">
            Agents
          </h2>
          <div className="space-y-1.5">
            {agents?.map((agent) => (
              <Link
                key={agent.id}
                to={`/agents/${agent.id}`}
                className="flex items-center gap-3 px-3 py-2 rounded-lg hover:bg-sera-surface-hover transition-colors"
              >
                <span
                  className={cn(
                    'w-2 h-2 rounded-full flex-shrink-0',
                    agent.status === 'running'
                      ? 'bg-sera-success'
                      : agent.status === 'error'
                        ? 'bg-sera-error'
                        : 'bg-sera-text-dim'
                  )}
                />
                <span className="text-sm text-sera-text flex-1">
                  {agent.display_name ?? agent.name}
                </span>
                <span className="text-[11px] text-sera-text-muted">{agent.template_ref}</span>
                <span
                  className={cn(
                    'text-[11px] px-1.5 py-0.5 rounded',
                    agent.status === 'running'
                      ? 'text-sera-success bg-sera-success/10'
                      : agent.status === 'error'
                        ? 'text-sera-error bg-sera-error/10'
                        : 'text-sera-text-dim bg-sera-surface'
                  )}
                >
                  {agent.status}
                </span>
              </Link>
            ))}
          </div>
        </section>
      )}

      {/* Recent sessions */}
      <RecentSessions />

      {/* Quick actions */}
      <div className="flex items-center gap-3 flex-wrap">
        <Link
          to="/chat"
          className="inline-flex items-center gap-2 px-4 py-2 text-xs font-medium rounded-lg bg-sera-accent text-white hover:bg-sera-accent/90 transition-colors"
        >
          <MessageSquare size={14} /> Open Chat
        </Link>
        <Link
          to="/agents/new"
          className="inline-flex items-center gap-2 px-4 py-2 text-xs font-medium rounded-lg border border-sera-border hover:bg-sera-surface transition-colors text-sera-text"
        >
          <Bot size={14} /> Create Agent
        </Link>
        {errored > 0 && (
          <Link
            to="/agents"
            className="inline-flex items-center gap-2 px-4 py-2 text-xs font-medium rounded-lg bg-sera-error/10 border border-sera-error/20 text-sera-error hover:bg-sera-error/20 transition-colors"
          >
            <AlertTriangle size={14} /> {errored} agent{errored > 1 ? 's' : ''} in error state
          </Link>
        )}
      </div>
    </div>
  );
}
