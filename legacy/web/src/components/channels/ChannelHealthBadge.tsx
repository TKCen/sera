/* eslint-disable react-refresh/only-export-components */
import { Badge } from '@/components/ui/badge';
import { useChannelHealth } from '@/hooks/useNotifications';

export function typeBadge(type: string) {
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

export function ChannelHealthBadge({ channelId }: { channelId: string }) {
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
