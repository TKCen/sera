import { useState } from 'react';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogClose,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { useCreateChannel } from '@/hooks/useNotifications';
import { useAgents } from '@/hooks/useAgents';
import type { CreateChannelPayload } from '@/lib/api/notifications';

const CHANNEL_TYPES = ['webhook', 'email', 'discord', 'discord-chat', 'slack'] as const;
type ChannelType = (typeof CHANNEL_TYPES)[number];

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
};

export function CreateChannelDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [name, setName] = useState('');
  const [type, setType] = useState<ChannelType>('webhook');
  const [configValues, setConfigValues] = useState<Record<string, string>>({});
  const create = useCreateChannel();
  const { data: agents } = useAgents();

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
    const payload: CreateChannelPayload = { name: name.trim(), type, config: buildConfig() };
    create.mutate(payload, {
      onSuccess: () => {
        onClose();
        setName('');
        setConfigValues({});
      },
    });
  }

  return (
    <Dialog open={open} onOpenChange={(o: boolean) => !o && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Add Channel</DialogTitle>
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
          <Button onClick={submit} disabled={create.isPending || !name.trim()}>
            {create.isPending ? 'Creating…' : 'Create'}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
