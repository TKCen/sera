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
  Plus,
} from 'lucide-react';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';
import { Alert } from '@/components/ui/alert';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/card';
import { Tooltip } from '@/components/ui/tooltip';
import { useQuery } from '@tanstack/react-query';
import { useAgents } from '@/hooks/useAgents';
import { useHealthDetail } from '@/hooks/useHealth';
import { useCircles } from '@/hooks/useCircles';
import { useSchedules } from '@/hooks/useSchedules';
import { request } from '@/lib/api/client';
import { cn, formatDistanceToNow } from '@/lib/utils';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { queryClient } from '@/lib/query-client';

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
  isLoading,
}: {
  label: string;
  value: string | number;
  icon: React.ComponentType<{ size?: number; className?: string }>;
  to: string;
  accent?: string;
  isLoading?: boolean;
}) {
  return (
    <Card className="p-0 overflow-hidden hover:border-sera-accent/40 transition-colors group">
      <Link
        to={to}
        className="block p-4"
        aria-label={`${label}: ${isLoading ? 'loading' : value}`}
      >
        <div className="flex items-center justify-between mb-2">
          <Icon
            size={18}
            className={cn(
              'text-sera-text-muted group-hover:text-sera-accent transition-colors',
              accent
            )}
            aria-hidden="true"
          />
        </div>
        {isLoading ? (
          <Skeleton className="h-8 w-16 mb-1" />
        ) : (
          <div className="text-2xl font-bold text-sera-text">{value}</div>
        )}
        <div className="text-xs text-sera-text-muted mt-0.5">{label}</div>
      </Link>
    </Card>
  );
}

function HealthBanner({ status }: { status: 'healthy' | 'degraded' | 'unhealthy' | string }) {
  const commonClasses =
    'flex items-center gap-2 px-3 py-2 rounded-lg border text-xs transition-colors cursor-help';

  let banner;
  let description;

  if (status === 'healthy') {
    description = 'All systems are functioning normally.';
    banner = (
      <div
        className={cn(commonClasses, 'bg-sera-success/10 border-sera-success/20 text-sera-success')}
        role="status"
        aria-live="polite"
      >
        <CheckCircle size={14} aria-hidden="true" /> All systems operational
      </div>
    );
  } else if (status === 'degraded') {
    description = 'Some services are experiencing issues but the system remains partially functional.';
    banner = (
      <div
        className={cn(commonClasses, 'bg-yellow-500/10 border-yellow-500/20 text-yellow-400')}
        role="status"
        aria-live="polite"
      >
        <AlertTriangle size={14} aria-hidden="true" /> Some services degraded
      </div>
    );
  } else {
    description = 'The system is experiencing critical failures.';
    banner = (
      <div
        className={cn(commonClasses, 'bg-sera-error/10 border-sera-error/20 text-sera-error')}
        role="status"
        aria-live="polite"
      >
        <XCircle size={14} aria-hidden="true" /> System unhealthy
      </div>
    );
  }

  return (
    <Tooltip content={description}>
      {banner}
    </Tooltip>
  );
}

function RecentSessions() {
  const { data: sessions, isLoading, error } = useSessions();
  const recent = (sessions ?? [])
    .sort((a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime())
    .slice(0, 5);

  if (isLoading) {
    return (
      <Card className="p-0" aria-busy="true">
        <CardHeader className="flex flex-row items-center justify-between space-y-0 p-4">
          <Skeleton className="h-4 w-32" />
        </CardHeader>
        <CardContent className="p-4 pt-0">
          <div className="space-y-1.5">
            {Array.from({ length: 3 }).map((_, i) => (
              <Skeleton key={i} className="h-10 w-full" />
            ))}
          </div>
        </CardContent>
      </Card>
    );
  }

  if (error) {
    return (
      <Card className="p-4 border-sera-error/30">
        <Alert variant="error" title="Failed to load recent sessions">
          {error.message}
        </Alert>
      </Card>
    );
  }

  if (!recent.length) {
    return (
      <Card className="p-4">
        <EmptyState
          icon={<MessageSquare size={24} />}
          title="No recent sessions"
          description="Your recent chat sessions with agents will appear here."
          action={
            <Button variant="outline" size="sm" asChild>
              <Link to="/chat">Start Chatting</Link>
            </Button>
          }
        />
      </Card>
    );
  }

  return (
    <Card className="p-0">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 p-4 pb-3">
        <CardTitle className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider">
          Recent Sessions
        </CardTitle>
        <Link
          to="/chat"
          className="text-[11px] text-sera-accent hover:underline"
          aria-label="View all chat sessions"
        >
          View all
        </Link>
      </CardHeader>
      <CardContent className="p-4 pt-0">
        <ul className="space-y-1.5">
          {recent.map((s) => (
            <li key={s.id}>
              <Link
                to="/chat"
                className="flex items-center gap-3 px-3 py-2 rounded-lg hover:bg-sera-surface-hover transition-colors"
              >
                <MessageSquare
                  size={13}
                  className="text-sera-text-muted flex-shrink-0"
                  aria-hidden="true"
                />
                <span className="text-sm text-sera-text flex-1 truncate">{s.title}</span>
                <span className="text-[10px] text-sera-text-dim">{s.agentName}</span>
                <span className="text-[10px] text-sera-text-dim">
                  {s.messageCount} msg{s.messageCount !== 1 ? 's' : ''}
                </span>
                <span className="text-[10px] text-sera-text-dim">
                  {formatDistanceToNow(s.updatedAt)}
                </span>
              </Link>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}

export default function DashboardPage() {
  const { data: agents, isLoading: agentsLoading, error: agentsError } = useAgents();
  const { data: health, isLoading: healthLoading, error: healthError } = useHealthDetail();
  const { data: circles, isLoading: circlesLoading, error: circlesError } = useCircles();
  const { data: schedules, isLoading: schedulesLoading, error: schedulesError } = useSchedules({});

  const running = agents?.filter((a) => a.status === 'running').length ?? 0;
  const errored = agents?.filter((a) => a.status === 'error').length ?? 0;
  const totalAgents = agents?.length ?? 0;
  const activeSchedules = schedules?.filter((s) => s.status === 'active').length ?? 0;

  const handleReset = () => {
    void queryClient.invalidateQueries();
  };

  return (
    <main className="p-8 max-w-5xl mx-auto space-y-6">
      <ErrorBoundary
        fallbackMessage="The dashboard header encountered an error."
        onReset={handleReset}
      >
        <div className="flex items-center justify-between">
          <div>
            <h1 className="sera-page-title">Dashboard</h1>
            <p className="text-sm text-sera-text-muted mt-1">SERA platform overview</p>
          </div>
          {!healthLoading && health && !healthError && <HealthBanner status={health.status} />}
          {healthLoading && <Skeleton className="h-8 w-48" />}
          {healthError && (
            <div className="flex items-center gap-2 px-3 py-2 rounded-lg border border-sera-error/20 bg-sera-error/10 text-sera-error text-xs">
              <XCircle size={14} aria-hidden="true" /> Health status unavailable
            </div>
          )}
        </div>
      </ErrorBoundary>

      {/* Stats grid */}
      <ErrorBoundary
        fallbackMessage="The dashboard statistics encountered an error."
        onReset={handleReset}
      >
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
          {agentsError ? (
            <>
              <div className="sera-card-static p-4 border-sera-error/30 text-sera-error text-xs">
                Failed to load agents
              </div>
              <div className="sera-card-static p-4 border-sera-error/30 text-sera-error text-xs">
                Failed to load status
              </div>
            </>
          ) : (
            <>
              <StatCard
                label="Total agents"
                value={totalAgents}
                icon={Bot}
                to="/agents"
                isLoading={agentsLoading}
              />
              <StatCard
                label="Running"
                value={running}
                icon={Activity}
                to="/agents"
                accent="text-sera-success"
                isLoading={agentsLoading}
              />
            </>
          )}
          {circlesError ? (
            <div className="sera-card-static p-4 border-sera-error/30 text-sera-error text-xs">
              Failed to load circles
            </div>
          ) : (
            <StatCard
              label="Circles"
              value={circles?.length ?? 0}
              icon={Circle}
              to="/circles"
              isLoading={circlesLoading}
            />
          )}
          {schedulesError ? (
            <div className="sera-card-static p-4 border-sera-error/30 text-sera-error text-xs">
              Failed to load schedules
            </div>
          ) : (
            <StatCard
              label="Active schedules"
              value={activeSchedules}
              icon={Clock}
              to="/schedules"
              isLoading={schedulesLoading}
            />
          )}
        </div>
      </ErrorBoundary>

      {/* Agent status breakdown */}
      <ErrorBoundary
        fallbackMessage="The agent status breakdown encountered an error."
        onReset={handleReset}
      >
        <Card className="p-0">
          <CardHeader className="flex flex-row items-center justify-between space-y-0 p-4 pb-3">
            <CardTitle className="text-xs font-semibold text-sera-text-dim uppercase tracking-wider">
              Agents
            </CardTitle>
            <Link
              to="/agents"
              className="text-[11px] text-sera-accent hover:underline"
              aria-label="View all agents"
            >
              View all
            </Link>
          </CardHeader>

          <CardContent className="p-4 pt-0">
            {agentsLoading ? (
              <div className="space-y-1.5" aria-busy="true">
                {Array.from({ length: 3 }).map((_, i) => (
                  <Skeleton key={i} className="h-10 w-full" />
                ))}
              </div>
            ) : agentsError ? (
              <Alert variant="error" title="Failed to load agents">
                {agentsError.message}
              </Alert>
            ) : agents && agents.length > 0 ? (
              <ul className="space-y-1.5">
                {agents.map((agent) => (
                  <li key={agent.id}>
                    <Link
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
                        aria-hidden="true"
                      />
                      <span className="text-sm text-sera-text flex-1">
                        {agent.display_name ?? agent.name}
                      </span>
                      <span className="text-[11px] text-sera-text-muted">{agent.template_ref}</span>
                      <Badge
                        variant={
                          agent.status === 'running'
                            ? 'success'
                            : agent.status === 'error'
                              ? 'error'
                              : 'default'
                        }
                      >
                        {agent.status}
                      </Badge>
                    </Link>
                  </li>
                ))}
              </ul>
            ) : (
              <EmptyState
                icon={<Bot size={24} />}
                title="No agents found"
                description="Create your first agent to start using SERA."
                action={
                  <Button size="sm" asChild>
                    <Link to="/agents/new">
                      <Plus size={14} className="mr-2" />
                      Create Agent
                    </Link>
                  </Button>
                }
              />
            )}
          </CardContent>
        </Card>
      </ErrorBoundary>

      {/* Recent sessions */}
      <ErrorBoundary
        fallbackMessage="Recent sessions could not be displayed."
        onReset={handleReset}
      >
        <RecentSessions />
      </ErrorBoundary>

      {/* Quick actions */}
      <nav className="flex items-center gap-3 flex-wrap" aria-label="Quick actions">
        <Button size="sm" asChild>
          <Link to="/chat">
            <MessageSquare size={14} aria-hidden="true" /> Open Chat
          </Link>
        </Button>
        <Button variant="outline" size="sm" asChild>
          <Link to="/agents/new">
            <Plus size={14} aria-hidden="true" /> Create Agent
          </Link>
        </Button>
        {!agentsLoading && errored > 0 && (
          <Button variant="danger" size="sm" asChild>
            <Link to="/agents">
              <AlertTriangle size={14} aria-hidden="true" /> {errored} agent{errored > 1 ? 's' : ''}{' '}
              in error state
            </Link>
          </Button>
        )}
      </nav>
    </main>
  );
}
