import { useState } from 'react';
import { Plus, Trash2, Send, Radio, ChevronDown, ChevronRight } from 'lucide-react';
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
  useDeleteChannel,
  useTestChannel,
  useCreateRoutingRule,
  useDeleteRoutingRule,
} from '@/hooks/useNotifications';
import type { NotificationChannel, CreateChannelPayload } from '@/lib/api/notifications';
import { useAuth } from '@/contexts/AuthContext';
import { ForbiddenView } from '@/views/ForbiddenView';

const CHANNEL_TYPES = ['webhook', 'email', 'discord', 'slack'] as const;
type ChannelType = (typeof CHANNEL_TYPES)[number];

const SEVERITY_OPTIONS = ['info', 'warning', 'critical'] as const;

// ── Config field definitions per channel type ────────────────────────────────

const CONFIG_FIELDS: Record<ChannelType, Array<{ key: string; label: string; type?: string; placeholder?: string }>> = {
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
    { key: 'webhookUrl', label: 'Discord Webhook URL', placeholder: 'https://discord.com/api/webhooks/...' },
    { key: 'botToken', label: 'Bot Token (optional)', type: 'password' },
    { key: 'approvalChannelId', label: 'Approval Channel ID (optional)' },
  ],
  slack: [
    { key: 'webhookUrl', label: 'Slack Incoming Webhook URL', placeholder: 'https://hooks.slack.com/...' },
    { key: 'botToken', label: 'Bot Token (optional)', type: 'password' },
    { key: 'appToken', label: 'App Token (optional)', type: 'password' },
    { key: 'signingSecret', label: 'Signing Secret (optional)', type: 'password' },
  ],
};

function typeBadge(type: string) {
  const colors: Record<string, 'default' | 'success' | 'warning'> = {
    webhook: 'default',
    email: 'default',
    discord: 'success',
    slack: 'warning',
  };
  return <Badge variant={colors[type] ?? 'default'}>{type}</Badge>;
}

// ── Create Channel Dialog ────────────────────────────────────────────────────

function CreateChannelDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [name, setName] = useState('');
  const [type, setType] = useState<ChannelType>('webhook');
  const [configValues, setConfigValues] = useState<Record<string, string>>({});
  const create = useCreateChannel();

  function setField(key: string, value: string) {
    setConfigValues((prev) => ({ ...prev, [key]: value }));
  }

  function buildConfig(): Record<string, unknown> {
    const cfg: Record<string, unknown> = {};
    for (const field of CONFIG_FIELDS[type]) {
      const v = configValues[field.key];
      if (!v) continue;
      if (field.key === 'smtpPort') cfg[field.key] = parseInt(v, 10);
      else if (field.key === 'to') cfg[field.key] = v.split(',').map((s) => s.trim());
      else cfg[field.key] = v;
    }
    return cfg;
  }

  function submit() {
    if (!name.trim()) return;
    const payload: CreateChannelPayload = { name: name.trim(), type, config: buildConfig() };
    create.mutate(payload, { onSuccess: () => { onClose(); setName(''); setConfigValues({}); } });
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Add Channel</DialogTitle>
        </DialogHeader>

        <div className="space-y-3">
          <div>
            <label className="sera-label">Name</label>
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="ops-discord" />
          </div>

          <div>
            <label className="sera-label">Type</label>
            <select
              className="w-full rounded border border-sera-border bg-sera-surface text-sera-text px-3 py-2 text-sm"
              value={type}
              onChange={(e) => { setType(e.target.value as ChannelType); setConfigValues({}); }}
            >
              {CHANNEL_TYPES.map((t) => (
                <option key={t} value={t}>{t}</option>
              ))}
            </select>
          </div>

          {CONFIG_FIELDS[type].map((field) => (
            <div key={field.key}>
              <label className="sera-label">{field.label}</label>
              <Input
                type={field.type ?? 'text'}
                placeholder={field.placeholder}
                value={configValues[field.key] ?? ''}
                onChange={(e) => setField(field.key, e.target.value)}
              />
            </div>
          ))}
        </div>

        <div className="flex justify-end gap-2 mt-4">
          <DialogClose asChild>
            <Button variant="ghost">Cancel</Button>
          </DialogClose>
          <Button onClick={submit} disabled={create.isPending || !name.trim()}>
            {create.isPending ? 'Creating…' : 'Create'}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

// ── Create Routing Rule Dialog ───────────────────────────────────────────────

function CreateRuleDialog({
  open,
  channels,
  onClose,
}: {
  open: boolean;
  channels: NotificationChannel[];
  onClose: () => void;
}) {
  const [eventType, setEventType] = useState('*');
  const [minSeverity, setMinSeverity] = useState('info');
  const [selectedChannels, setSelectedChannels] = useState<string[]>([]);
  const create = useCreateRoutingRule();

  function toggle(id: string) {
    setSelectedChannels((prev) =>
      prev.includes(id) ? prev.filter((c) => c !== id) : [...prev, id],
    );
  }

  function submit() {
    if (!eventType.trim() || selectedChannels.length === 0) return;
    create.mutate(
      { eventType: eventType.trim(), channelIds: selectedChannels, minSeverity },
      { onSuccess: () => { onClose(); setEventType('*'); setSelectedChannels([]); } },
    );
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Add Routing Rule</DialogTitle>
        </DialogHeader>

        <div className="space-y-3">
          <div>
            <label className="sera-label">Event Type Pattern</label>
            <Input
              value={eventType}
              onChange={(e) => setEventType(e.target.value)}
              placeholder="permission.* or * or agent.crashed"
            />
            <p className="text-[11px] text-sera-text-dim mt-1">Supports * wildcard, e.g. permission.*</p>
          </div>

          <div>
            <label className="sera-label">Minimum Severity</label>
            <select
              className="w-full rounded border border-sera-border bg-sera-surface text-sera-text px-3 py-2 text-sm"
              value={minSeverity}
              onChange={(e) => setMinSeverity(e.target.value)}
            >
              {SEVERITY_OPTIONS.map((s) => (
                <option key={s} value={s}>{s}</option>
              ))}
            </select>
          </div>

          <div>
            <label className="sera-label">Target Channels</label>
            <div className="space-y-1 max-h-40 overflow-y-auto border border-sera-border rounded p-2">
              {channels.map((ch) => (
                <label key={ch.id} className="flex items-center gap-2 cursor-pointer text-sm text-sera-text">
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
            disabled={create.isPending || !eventType.trim() || selectedChannels.length === 0}
          >
            {create.isPending ? 'Creating…' : 'Create'}
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

  const [addChannelOpen, setAddChannelOpen] = useState(false);
  const [addRuleOpen, setAddRuleOpen] = useState(false);
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
          <Button size="sm" onClick={() => setAddChannelOpen(true)}>
            <Plus size={14} className="mr-1" /> Add Channel
          </Button>
        </div>

        {channelsLoading ? (
          <div className="space-y-2">
            {[...Array(2)].map((_, i) => <Skeleton key={i} className="h-12 w-full" />)}
          </div>
        ) : channels && channels.length > 0 ? (
          <div className="border border-sera-border rounded-lg overflow-hidden divide-y divide-sera-border">
            {channels.map((ch) => (
              <div key={ch.id} className="flex items-center gap-3 px-4 py-3 bg-sera-surface hover:bg-sera-surface-hover">
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium text-sera-text">{ch.name}</span>
                    {typeBadge(ch.type)}
                    {!ch.enabled && <Badge variant="default">disabled</Badge>}
                  </div>
                </div>
                <div className="flex items-center gap-2 flex-shrink-0">
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
          <h2 className="text-sm font-semibold text-sera-text uppercase tracking-wide">Routing Rules</h2>
          <Button size="sm" onClick={() => setAddRuleOpen(true)} disabled={!channels || channels.length === 0}>
            <Plus size={14} className="mr-1" /> Add Rule
          </Button>
        </div>

        {rulesLoading ? (
          <div className="space-y-2">
            {[...Array(2)].map((_, i) => <Skeleton key={i} className="h-12 w-full" />)}
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
                    <code className="text-sm font-mono text-sera-accent">{rule.eventType}</code>
                    <Badge variant="default">{rule.minSeverity}+</Badge>
                    <span className="text-xs text-sera-text-dim ml-auto">
                      → {ruleChannels.map((c) => c.name).join(', ') || `${rule.channelIds.length} channel(s)`}
                    </span>
                  </button>
                  {expanded && (
                    <div className="px-4 pb-3 flex items-center justify-between">
                      <div className="text-xs text-sera-text-dim space-y-1">
                        <div>Channels: {ruleChannels.map((c) => c.name).join(', ') || rule.channelIds.join(', ')}</div>
                        {rule.filter && <div>Filter: <code>{JSON.stringify(rule.filter)}</code></div>}
                      </div>
                      <Button
                        size="sm"
                        variant="danger"
                        disabled={deleteRule.isPending}
                        onClick={() => deleteRule.mutate(rule.id)}
                      >
                        <Trash2 size={13} />
                      </Button>
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

      <CreateChannelDialog open={addChannelOpen} onClose={() => setAddChannelOpen(false)} />
      <CreateRuleDialog
        open={addRuleOpen}
        channels={channels ?? []}
        onClose={() => setAddRuleOpen(false)}
      />
    </div>
  );
}
