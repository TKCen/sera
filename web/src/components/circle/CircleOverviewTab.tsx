import { Link } from 'react-router';
import { Bot, Zap, Network, Trash2, Pencil, Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import type { CirclePartyModeConfig } from '@/lib/api/types';

interface CircleOverviewTabProps {
  agents: string[];
  partyMode: CirclePartyModeConfig | undefined;
  connections: Array<{
    circle: string;
    bridgeChannels?: string[];
    auth?: string | Record<string, unknown>;
  }>;
  onAddMember: () => void;
  onRemoveMember: (agent: string) => void;
  onEditPartyMode: () => void;
}

export function CircleOverviewTab({
  agents,
  partyMode,
  connections,
  onAddMember,
  onRemoveMember,
  onEditPartyMode,
}: CircleOverviewTabProps) {
  return (
    <div className="space-y-6">
      <section>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-sm font-semibold text-sera-text flex items-center gap-2">
            <Bot size={15} />
            Members ({agents.length})
          </h2>
          <Button size="sm" variant="outline" onClick={onAddMember}>
            <Plus size={12} /> Add Member
          </Button>
        </div>
        {agents.length === 0 ? (
          <p className="text-xs text-sera-text-dim">No agents in this circle.</p>
        ) : (
          <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
            {agents.map((agent) => (
              <div
                key={agent}
                className="sera-card-static rounded-lg px-4 py-3 flex items-center gap-3 group/member"
              >
                <Link
                  to={`/agents?search=${encodeURIComponent(agent)}`}
                  className="flex-1 flex items-center gap-3 min-w-0"
                >
                  <div className="h-8 w-8 rounded-full bg-sera-accent-soft flex items-center justify-center flex-shrink-0">
                    <Bot size={14} className="text-sera-accent" />
                  </div>
                  <div className="min-w-0">
                    <span className="text-sm font-medium text-sera-text truncate block">
                      {agent}
                    </span>
                  </div>
                </Link>
                <button
                  onClick={() => void onRemoveMember(agent)}
                  className="p-1 rounded text-sera-text-dim opacity-0 group-hover/member:opacity-100 hover:bg-sera-error/10 hover:text-sera-error transition-all"
                  title="Remove from circle"
                >
                  <Trash2 size={12} />
                </button>
              </div>
            ))}
          </div>
        )}
      </section>

      {partyMode && (
        <section>
          <div className="flex items-center justify-between mb-3">
            <h2 className="text-sm font-semibold text-sera-text flex items-center gap-2">
              <Zap size={15} />
              Party Mode
            </h2>
            <Button size="sm" variant="outline" onClick={onEditPartyMode}>
              <Pencil size={12} /> Edit
            </Button>
          </div>
          <div className="sera-card-static rounded-lg p-4 space-y-2">
            <div className="flex items-center gap-2">
              <span className="text-xs text-sera-text-muted w-28">Status</span>
              <Badge variant={partyMode.enabled ? 'success' : 'default'}>
                {partyMode.enabled ? 'Enabled' : 'Disabled'}
              </Badge>
            </div>
            {partyMode.orchestrator && (
              <div className="flex items-center gap-2">
                <span className="text-xs text-sera-text-muted w-28">Orchestrator</span>
                <span className="text-xs text-sera-text font-mono">{partyMode.orchestrator}</span>
              </div>
            )}
            {partyMode.selectionStrategy && (
              <div className="flex items-center gap-2">
                <span className="text-xs text-sera-text-muted w-28">Strategy</span>
                <Badge variant="accent">{partyMode.selectionStrategy}</Badge>
              </div>
            )}
          </div>
        </section>
      )}

      {connections.length > 0 && (
        <section>
          <h2 className="text-sm font-semibold text-sera-text mb-3 flex items-center gap-2">
            <Network size={15} />
            Connections ({connections.length})
          </h2>
          <div className="space-y-2">
            {connections.map((conn, i) => (
              <div
                key={i}
                className="sera-card-static rounded-lg px-4 py-3 flex items-center gap-3"
              >
                <Network size={14} className="text-sera-text-muted flex-shrink-0" />
                <div className="flex-1 min-w-0">
                  <Link
                    to={`/circles/${conn.circle}`}
                    className="text-sm font-medium text-sera-accent hover:underline"
                  >
                    {conn.circle}
                  </Link>
                  {conn.bridgeChannels && conn.bridgeChannels.length > 0 && (
                    <div className="flex items-center gap-1 mt-1 flex-wrap">
                      {conn.bridgeChannels.map((ch) => (
                        <Badge key={ch} variant="default" className="text-[10px]">
                          {ch}
                        </Badge>
                      ))}
                    </div>
                  )}
                </div>
                <Badge variant="default">
                  {typeof conn.auth === 'string' ? conn.auth : 'custom'}
                </Badge>
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
