import { useState } from 'react';
import { Plus, Trash2, Send, Radio, ChevronDown, ChevronRight, Edit2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import {
  useChannels,
  useRoutingRules,
  useDeleteChannel,
  useTestChannel,
  useDeleteRoutingRule,
} from '@/hooks/useNotifications';
import type {
  NotificationChannel,
  RoutingRule,
} from '@/hooks/useNotifications';
import { useAuth } from '@/hooks/useAuth';
import { ForbiddenView } from '@/views/ForbiddenView';
import { ChannelDialog } from '@/components/ChannelDialog';
import { RuleDialog } from '@/components/RuleDialog';

function typeBadge(type: string) {
  const colors: Record<string, 'default' | 'success' | 'warning' | 'accent'> = {
    webhook: 'default',
    email: 'default',
    discord: 'success',
    'discord-chat': 'accent',
    slack: 'warning',
  };
  return <Badge variant={colors[type] ?? 'default'}>{type}</Badge>;
}

// ── Main page ────────────────────────────────────────────────────────────────

export default function ChannelsPage() {
  const { roles } = useAuth();
  const isAdmin = roles.includes('admin');

  const { data: channels, isLoading: channelsLoading } = useChannels();
  const { data: rules, isLoading: rulesLoading } = useRoutingRules();
  const deleteChannel = useDeleteChannel();
  const testChannel = useTestChannel();
  const deleteRule = useDeleteRoutingRule();

  const [channelDialog, setChannelDialog] = useState<{ open: boolean; data?: NotificationChannel }>(
    {
      open: false,
    }
  );
  const [ruleDialog, setRuleDialog] = useState<{ open: boolean; data?: RoutingRule }>({
    open: false,
  });
  const [expandedRule, setExpandedRule] = useState<string | null>(null);

  if (!isAdmin) return <ForbiddenView />;

  return (
    <div className="p-6 space-y-8 max-w-4xl">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold text-sera-text flex items-center gap-2">
            <Radio size={20} className="text-sera-accent" />
            Integration Channels
          </h1>
          <p className="text-sm text-sera-text-dim mt-1">
            Configure outbound notification channels and event routing rules.
          </p>
        </div>
      </div>

      {/* ── Channels ─────────────────────────────────────────────────────── */}
      <section>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-sm font-semibold text-sera-text uppercase tracking-wide">Channels</h2>
          <Button size="sm" onClick={() => setChannelDialog({ open: true })}>
            <Plus size={14} className="mr-1" /> Add Channel
          </Button>
        </div>

        {channelsLoading ? (
          <div className="space-y-2">
            {[...Array(2)].map((_, i) => (
              <Skeleton key={i} className="h-12 w-full" />
            ))}
          </div>
        ) : channels && channels.length > 0 ? (
          <div className="border border-sera-border rounded-lg overflow-hidden divide-y divide-sera-border">
            {channels.map((ch) => (
              <div
                key={ch.id}
                className="flex items-center gap-3 px-4 py-3 bg-sera-surface hover:bg-sera-surface-hover"
              >
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium text-sera-text">{ch.name}</span>
                    {typeBadge(ch.type)}
                    {!ch.enabled && <Badge variant="default">disabled</Badge>}
                  </div>
                  {ch.description && (
                    <p className="text-xs text-sera-text-dim truncate">{ch.description}</p>
                  )}
                </div>
                <div className="flex items-center gap-2 flex-shrink-0">
                  <Button
                    size="sm"
                    variant="ghost"
                    title="Edit channel"
                    onClick={() => setChannelDialog({ open: true, data: ch })}
                  >
                    <Edit2 size={13} />
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    title="Send test notification"
                    disabled={testChannel.isPending}
                    onClick={() => testChannel.mutate(ch.id)}
                  >
                    <Send size={13} />
                  </Button>
                  <Button
                    size="sm"
                    variant="danger"
                    title="Delete channel"
                    disabled={deleteChannel.isPending}
                    onClick={() => deleteChannel.mutate(ch.id)}
                  >
                    <Trash2 size={13} />
                  </Button>
                </div>
              </div>
            ))}
          </div>
        ) : (
          <p className="text-sm text-sera-text-dim border border-dashed border-sera-border rounded-lg p-6 text-center">
            No channels configured. Add one to start routing notifications.
          </p>
        )}
      </section>

      {/* ── Routing Rules ─────────────────────────────────────────────────── */}
      <section>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-sm font-semibold text-sera-text uppercase tracking-wide">
            Routing Rules
          </h2>
          <Button
            size="sm"
            onClick={() => setRuleDialog({ open: true })}
            disabled={!channels || channels.length === 0}
          >
            <Plus size={14} className="mr-1" /> Add Rule
          </Button>
        </div>

        {rulesLoading ? (
          <div className="space-y-2">
            {[...Array(2)].map((_, i) => (
              <Skeleton key={i} className="h-12 w-full" />
            ))}
          </div>
        ) : rules && rules.length > 0 ? (
          <div className="border border-sera-border rounded-lg overflow-hidden divide-y divide-sera-border">
            {rules.map((rule) => {
              const expanded = expandedRule === rule.id;
              const ruleChannels = channels?.filter((c) => rule.channelIds.includes(c.id)) ?? [];
              return (
                <div key={rule.id} className="bg-sera-surface">
                  <button
                    className="w-full flex items-center gap-3 px-4 py-3 text-left hover:bg-sera-surface-hover"
                    onClick={() => setExpandedRule(expanded ? null : rule.id)}
                  >
                    {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                    <div className="flex items-center gap-2 flex-1">
                      <code className="text-sm font-mono text-sera-accent">{rule.eventType}</code>
                      <Badge variant="default">{rule.minSeverity}+</Badge>
                      <Badge
                        variant="default"
                        className="text-[10px] py-0 h-4 border border-sera-border"
                      >
                        P{rule.priority}
                      </Badge>
                      {!rule.enabled && <Badge variant="default">disabled</Badge>}
                    </div>
                    <span className="text-xs text-sera-text-dim ml-auto">
                      →{' '}
                      {ruleChannels.map((c) => c.name).join(', ') ||
                        `${rule.channelIds.length} channel(s)`}
                    </span>
                  </button>
                  {expanded && (
                    <div className="px-4 pb-3 flex items-center justify-between">
                      <div className="text-xs text-sera-text-dim space-y-1">
                        <div>
                          Channels:{' '}
                          {ruleChannels.map((c) => c.name).join(', ') || rule.channelIds.join(', ')}
                        </div>
                        {rule.targetAgentId && (
                          <div>
                            Target Agent: <code>{rule.targetAgentId}</code>
                          </div>
                        )}
                        {rule.filter && (
                          <div>
                            Filter: <code>{JSON.stringify(rule.filter)}</code>
                          </div>
                        )}
                      </div>
                      <div className="flex gap-2">
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() => setRuleDialog({ open: true, data: rule })}
                        >
                          <Edit2 size={13} />
                        </Button>
                        <Button
                          size="sm"
                          variant="danger"
                          disabled={deleteRule.isPending}
                          onClick={() => deleteRule.mutate(rule.id)}
                        >
                          <Trash2 size={13} />
                        </Button>
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        ) : (
          <p className="text-sm text-sera-text-dim border border-dashed border-sera-border rounded-lg p-6 text-center">
            No routing rules. Rules control which events reach which channels.
          </p>
        )}
      </section>

      <ChannelDialog
        open={channelDialog.open}
        initialData={channelDialog.data}
        onClose={() => setChannelDialog({ open: false })}
      />
      <RuleDialog
        open={ruleDialog.open}
        initialData={ruleDialog.data}
        channels={channels ?? []}
        onClose={() => setRuleDialog({ open: false })}
      />
    </div>
  );
}
