import { useState, useEffect } from 'react';
import { Plus, Trash2, Send, Radio, ChevronDown, ChevronRight, Edit2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogClose,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import {
  useChannels,
  useRoutingRules,
  useCreateChannel,
  useUpdateChannel,
  useDeleteChannel,
  useTestChannel,
  useChannelHealth,
  useCreateRoutingRule,
  useUpdateRoutingRule,
  useDeleteRoutingRule,
} from '@/hooks/useNotifications';
import type {
  NotificationChannel,
  CreateChannelPayload,
  RoutingRule,
} from '@/lib/api/notifications';
import { useAuth } from '@/hooks/useAuth';
import { useAgents } from '@/hooks/useAgents';
import { ForbiddenView } from '@/views/ForbiddenView';

const CHANNEL_TYPES = [
  'webhook',
  'email',
  'discord',
  'discord-chat',
  'slack',
  'telegram',
  'whatsapp',
] as const;
type ChannelType = (typeof CHANNEL_TYPES)[number];

const SEVERITY_OPTIONS = ['info', 'warning', 'critical'] as const;

// ── Config field definitions per channel type ────────────────────────────────

const CONFIG_FIELDS: Record<
  ChannelType,
  Array<{ key: string; label: string; type?: string; placeholder?: string }>
> = {
  webhook: [
    { key: 'url', label: 'Webhook URL', placeholder: 'https://example.com/hook' },
    { key: 'secret', label: 'Signing Secret (optional)', type: 'password' },
    { key: 'timeout', label: 'Timeout ms (default 10000)', placeholder: '10000' },
  ],
  email: [
    { key: 'smtpHost', label: 'SMTP Host', placeholder: 'smtp.example.com' },
    { key: 'smtpPort', label: 'SMTP Port', placeholder: '587' },
    { key: 'smtpUser', label: 'SMTP User' },
    { key: 'smtpPassword', label: 'SMTP Password', type: 'password' },
    { key: 'from', label: 'From Address', placeholder: 'sera@example.com' },
    { key: 'to', label: 'To Addresses (comma-separated)', placeholder: 'ops@example.com' },
  ],
  discord: [
    {
      key: 'webhookUrl',
      label: 'Discord Webhook URL',
      placeholder: 'https://discord.com/api/webhooks/...',
    },
    { key: 'botToken', label: 'Bot Token (optional)', type: 'password' },
    { key: 'approvalChannelId', label: 'Approval Channel ID (optional)' },
  ],
  'discord-chat': [
    {
      key: 'botToken',
      label: 'Discord Bot Token',
      type: 'password',
      placeholder: 'Bot token from discord.dev',
    },
    {
      key: 'applicationId',
      label: 'Application ID',
      placeholder: 'From discord.dev application page',
    },
    { key: 'targetAgentId', label: 'Target Agent', type: 'agent-select' },
    {
      key: 'allowedGuilds',
      label: 'Allowed Guild IDs (comma-separated)',
      placeholder: 'Leave empty to allow all guilds',
    },
    {
      key: 'allowedUsers',
      label: 'Allowed User IDs (comma-separated)',
      placeholder: 'Leave empty to allow all users',
    },
    { key: 'allowDMs', label: 'Allow Direct Messages', type: 'checkbox' },
    { key: 'allowMentions', label: 'Respond to @Mentions in Guilds', type: 'checkbox' },
  ],
  slack: [
    {
      key: 'webhookUrl',
      label: 'Slack Incoming Webhook URL',
      placeholder: 'https://hooks.slack.com/...',
    },
    { key: 'botToken', label: 'Bot Token (optional)', type: 'password' },
    { key: 'appToken', label: 'App Token (optional)', type: 'password' },
    { key: 'signingSecret', label: 'Signing Secret (optional)', type: 'password' },
  ],
  telegram: [
    {
      key: 'botToken',
      label: 'Telegram Bot Token',
      type: 'password',
      placeholder: 'From @BotFather',
    },
    { key: 'targetAgentId', label: 'Target Agent', type: 'agent-select' },
    {
      key: 'allowedUsers',
      label: 'Allowed User IDs (comma-separated)',
      placeholder: 'Leave empty for all',
    },
  ],
  whatsapp: [
    { key: 'accessToken', label: 'Meta Cloud API Access Token', type: 'password' },
    { key: 'phoneNumberId', label: 'Phone Number ID', placeholder: 'From Meta Business Suite' },
    { key: 'verifyToken', label: 'Webhook Verify Token', placeholder: 'Custom verify token' },
    { key: 'targetAgentId', label: 'Target Agent', type: 'agent-select' },
  ],
};

function typeBadge(type: string) {
  const colors: Record<string, 'default' | 'success' | 'warning' | 'accent'> = {
    webhook: 'default',
    email: 'default',
    discord: 'success',
    'discord-chat': 'accent',
    slack: 'warning',
    telegram: 'accent',
    whatsapp: 'success',
  };
  return <Badge variant={colors[type] ?? 'default'}>{type}</Badge>;
}

function ChannelHealthBadge({ channelId }: { channelId: string }) {
  const { data: health, isLoading } = useChannelHealth(channelId);

  let color = 'bg-sera-text-dim';
  let title = 'Unknown';
  let pulse = false;

  if (isLoading) {
    title = 'Checking...';
    pulse = true;
  } else if (health?.healthy) {
    color = 'bg-green-500';
    title = 'Healthy';
  } else if (health) {
    color = 'bg-yellow-500';
    title = health.error ?? 'Degraded';
  }

  return (
    <span
      className={`inline-block w-2 h-2 rounded-full ${color}${pulse ? ' animate-pulse' : ''}`}
      title={title}
    />
  );
}

// ── Channel Dialog ───────────────────────────────────────────────────────────

function ChannelDialog({
  open,
  onClose,
  initialData,
}: {
  open: boolean;
  onClose: () => void;
  initialData?: NotificationChannel;
}) {
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [type, setType] = useState<ChannelType>('webhook');
  const [enabled, setEnabled] = useState(true);
  const [configValues, setConfigValues] = useState<Record<string, string>>({});

  const create = useCreateChannel();
  const update = useUpdateChannel();
  const { data: agents } = useAgents();

  useEffect(() => {
    if (initialData) {
      setName(initialData.name);
      setDescription(initialData.description || '');
      setType(initialData.type as ChannelType);
      setEnabled(initialData.enabled);
      const values: Record<string, string> = {};
      for (const [k, v] of Object.entries(initialData.config)) {
        values[k] = String(v);
      }
      setConfigValues(values);
    } else {
      setName('');
      setDescription('');
      setType('webhook');
      setEnabled(true);
      setConfigValues({});
    }
  }, [initialData, open]);

  function setField(key: string, value: string) {
    setConfigValues((prev) => ({ ...prev, [key]: value }));
  }

  function buildConfig(): Record<string, unknown> {
    const cfg: Record<string, unknown> = {};
    for (const field of CONFIG_FIELDS[type]) {
      const v = configValues[field.key];
      if (field.type === 'checkbox') {
        cfg[field.key] = v === 'true';
        continue;
      }
      if (!v) continue;
      if (field.key === 'smtpPort') cfg[field.key] = parseInt(v, 10);
      else if (
        field.key === 'to' ||
        field.key === 'allowedGuilds' ||
        field.key === 'allowedUsers'
      ) {
        cfg[field.key] = v
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean);
      } else cfg[field.key] = v;
    }
    // Default checkbox values for discord-chat
    if (type === 'discord-chat') {
      if (!('allowDMs' in cfg)) cfg['allowDMs'] = true;
      if (!('allowMentions' in cfg)) cfg['allowMentions'] = true;
    }
    return cfg;
  }

  function submit() {
    if (!name.trim()) return;
    const payload: CreateChannelPayload = {
      name: name.trim(),
      description: description.trim() || undefined,
      type,
      config: buildConfig(),
      enabled,
    };

    if (initialData) {
      update.mutate(
        { id: initialData.id, data: payload },
        {
          onSuccess: () => {
            onClose();
          },
        }
      );
    } else {
      create.mutate(payload, {
        onSuccess: () => {
          onClose();
        },
      });
    }
  }

  const isPending = create.isPending || update.isPending;

  return (
    <Dialog open={open} onOpenChange={(o: boolean) => !o && onClose()}>
      <DialogContent className="max-w-md max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{initialData ? 'Edit Channel' : 'Add Channel'}</DialogTitle>
        </DialogHeader>

        <div className="space-y-3">
          <div>
            <label className="sera-label">Name</label>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="ops-discord"
            />
          </div>

          <div>
            <label className="sera-label">Description (optional)</label>
            <Input
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Production alerts for ops team"
            />
          </div>

          {!initialData && (
            <div>
              <label className="sera-label">Type</label>
              <select
                className="w-full rounded border border-sera-border bg-sera-surface text-sera-text px-3 py-2 text-sm"
                value={type}
                onChange={(e) => {
                  setType(e.target.value as ChannelType);
                  setConfigValues({});
                }}
              >
                {CHANNEL_TYPES.map((t) => (
                  <option key={t} value={t}>
                    {t}
                  </option>
                ))}
              </select>
            </div>
          )}

          <div>
            <label className="flex items-center gap-2 py-1 cursor-pointer text-sm text-sera-text">
              <input
                type="checkbox"
                checked={enabled}
                onChange={(e) => setEnabled(e.target.checked)}
                className="accent-sera-accent"
              />
              Enabled
            </label>
          </div>

          {CONFIG_FIELDS[type].map((field) => (
            <div key={field.key}>
              {field.type === 'checkbox' ? (
                <label className="flex items-center gap-2 py-1 cursor-pointer text-sm text-sera-text">
                  <input
                    type="checkbox"
                    checked={(configValues[field.key] ?? 'true') === 'true'}
                    onChange={(e) => setField(field.key, String(e.target.checked))}
                    className="accent-sera-accent"
                  />
                  {field.label}
                </label>
              ) : field.type === 'agent-select' ? (
                <>
                  <label className="sera-label">{field.label}</label>
                  <select
                    value={configValues[field.key] ?? ''}
                    onChange={(e) => setField(field.key, e.target.value)}
                    className="sera-input text-sm"
                  >
                    <option value="">Select an agent…</option>
                    {agents?.map((a) => (
                      <option key={a.id} value={a.id}>
                        {a.display_name ?? a.name} ({a.id.substring(0, 8)})
                      </option>
                    ))}
                  </select>
                </>
              ) : (
                <>
                  <label className="sera-label">{field.label}</label>
                  <Input
                    type={field.type ?? 'text'}
                    placeholder={field.placeholder}
                    value={configValues[field.key] ?? ''}
                    onChange={(e) => setField(field.key, e.target.value)}
                  />
                </>
              )}
            </div>
          ))}
        </div>

        <div className="flex justify-end gap-2 mt-4">
          <DialogClose asChild>
            <Button variant="ghost">Cancel</Button>
          </DialogClose>
          <Button onClick={submit} disabled={isPending || !name.trim()}>
            {isPending ? 'Saving…' : initialData ? 'Update' : 'Create'}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

// ── Routing Rule Dialog ──────────────────────────────────────────────────────

function RuleDialog({
  open,
  channels,
  onClose,
  initialData,
}: {
  open: boolean;
  channels: NotificationChannel[];
  onClose: () => void;
  initialData?: RoutingRule;
}) {
  const [eventType, setEventType] = useState('*');
  const [minSeverity, setMinSeverity] = useState('info');
  const [priority, setPriority] = useState(0);
  const [enabled, setEnabled] = useState(true);
  const [targetAgentId, setTargetAgentId] = useState('');
  const [selectedChannels, setSelectedChannels] = useState<string[]>([]);

  const create = useCreateRoutingRule();
  const update = useUpdateRoutingRule();
  const { data: agents } = useAgents();

  useEffect(() => {
    if (initialData) {
      setEventType(initialData.eventType);
      setMinSeverity(initialData.minSeverity);
      setPriority(initialData.priority);
      setEnabled(initialData.enabled);
      setTargetAgentId(initialData.targetAgentId || '');
      setSelectedChannels(initialData.channelIds);
    } else {
      setEventType('*');
      setMinSeverity('info');
      setPriority(0);
      setEnabled(true);
      setTargetAgentId('');
      setSelectedChannels([]);
    }
  }, [initialData, open]);

  function toggle(id: string) {
    setSelectedChannels((prev) =>
      prev.includes(id) ? prev.filter((c) => c !== id) : [...prev, id]
    );
  }

  function submit() {
    if (!eventType.trim() || selectedChannels.length === 0) return;
    const payload = {
      eventType: eventType.trim(),
      channelIds: selectedChannels,
      minSeverity,
      priority,
      enabled,
      targetAgentId: targetAgentId || null,
    };

    if (initialData) {
      update.mutate(
        { id: initialData.id, data: payload },
        {
          onSuccess: () => {
            onClose();
          },
        }
      );
    } else {
      create.mutate(payload, {
        onSuccess: () => {
          onClose();
        },
      });
    }
  }

  const isPending = create.isPending || update.isPending;

  return (
    <Dialog open={open} onOpenChange={(o: boolean) => !o && onClose()}>
      <DialogContent className="max-w-md max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{initialData ? 'Edit Routing Rule' : 'Add Routing Rule'}</DialogTitle>
        </DialogHeader>

        <div className="space-y-3">
          <div className="flex gap-4">
            <div className="flex-1">
              <label className="sera-label">Event Type Pattern</label>
              <Input
                value={eventType}
                onChange={(e) => setEventType(e.target.value)}
                placeholder="permission.* or * or agent.crashed"
              />
            </div>
            <div className="w-24">
              <label className="sera-label">Priority</label>
              <Input
                type="number"
                value={priority}
                onChange={(e) => setPriority(parseInt(e.target.value, 10) || 0)}
              />
            </div>
          </div>
          <p className="text-[11px] text-sera-text-dim">Supports * wildcard, e.g. permission.*</p>

          <div className="flex gap-4">
            <div className="flex-1">
              <label className="sera-label">Minimum Severity</label>
              <select
                className="w-full rounded border border-sera-border bg-sera-surface text-sera-text px-3 py-2 text-sm"
                value={minSeverity}
                onChange={(e) => setMinSeverity(e.target.value)}
              >
                {SEVERITY_OPTIONS.map((s) => (
                  <option key={s} value={s}>
                    {s}
                  </option>
                ))}
              </select>
            </div>
            <div className="flex items-end pb-2">
              <label className="flex items-center gap-2 cursor-pointer text-sm text-sera-text">
                <input
                  type="checkbox"
                  checked={enabled}
                  onChange={(e) => setEnabled(e.target.checked)}
                  className="accent-sera-accent"
                />
                Enabled
              </label>
            </div>
          </div>

          <div>
            <label className="sera-label">Target Agent (optional)</label>
            <select
              value={targetAgentId}
              onChange={(e) => setTargetAgentId(e.target.value)}
              className="sera-input text-sm"
            >
              <option value="">Any Agent</option>
              {agents?.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.display_name ?? a.name} ({a.id.substring(0, 8)})
                </option>
              ))}
            </select>
          </div>

          <div>
            <label className="sera-label">Target Channels</label>
            <div className="space-y-1 max-h-40 overflow-y-auto border border-sera-border rounded p-2">
              {channels.map((ch) => (
                <label
                  key={ch.id}
                  className="flex items-center gap-2 cursor-pointer text-sm text-sera-text"
                >
                  <input
                    type="checkbox"
                    checked={selectedChannels.includes(ch.id)}
                    onChange={() => toggle(ch.id)}
                  />
                  {ch.name}
                  <span className="text-sera-text-dim text-xs">({ch.type})</span>
                </label>
              ))}
              {channels.length === 0 && (
                <p className="text-sera-text-dim text-xs">No channels configured yet.</p>
              )}
            </div>
          </div>
        </div>

        <div className="flex justify-end gap-2 mt-4">
          <DialogClose asChild>
            <Button variant="ghost">Cancel</Button>
          </DialogClose>
          <Button
            onClick={submit}
            disabled={isPending || !eventType.trim() || selectedChannels.length === 0}
          >
            {isPending ? 'Saving…' : initialData ? 'Update' : 'Create'}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
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
                    <ChannelHealthBadge channelId={ch.id} />
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
