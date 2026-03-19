import { Link } from 'react-router';
import { Bot, Plus } from 'lucide-react';
import { useAgents } from '@/hooks/useAgents';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';

export default function AgentsPage() {
  const { data: agents, isLoading } = useAgents();

  return (
    <div className="p-6">
      <div className="sera-page-header">
        <h1 className="sera-page-title">Agents</h1>
        <Button asChild size="sm">
          <Link to="/agents/new">
            <Plus size={14} />
            New Agent
          </Link>
        </Button>
      </div>

      {isLoading ? (
        <div className="space-y-3">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-16 rounded-xl" />
          ))}
        </div>
      ) : !agents?.length ? (
        <EmptyState
          icon={<Bot size={24} />}
          title="No agents yet"
          description="Create your first agent to get started."
          action={
            <Button asChild size="sm">
              <Link to="/agents/new">Create Agent</Link>
            </Button>
          }
        />
      ) : (
        <div className="space-y-2">
          {agents.map((agent) => (
            <Link
              key={agent.metadata.name}
              to={`/agents/${agent.metadata.name}`}
              className="sera-card flex items-center gap-4 px-4 py-3 block"
            >
              <div className="h-9 w-9 rounded-lg bg-sera-accent-soft flex items-center justify-center flex-shrink-0">
                <Bot size={16} className="text-sera-accent" />
              </div>
              <div className="flex-1 min-w-0">
                <div className="font-medium text-sm text-sera-text truncate">
                  {agent.metadata.displayName ?? agent.metadata.name}
                </div>
                <div className="text-xs text-sera-text-muted truncate">
                  {agent.metadata.name}
                </div>
              </div>
              <Badge variant="default">{agent.spec?.lifecycle?.mode ?? 'persistent'}</Badge>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
