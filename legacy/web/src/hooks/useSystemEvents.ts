import { useEffect, useState } from 'react';
import type { PublicationContext } from 'centrifuge';
import { useCentrifugoContext } from '@/hooks/useCentrifugo';

export interface SystemEvent {
  type: string;
  payload: unknown;
  timestamp: string;
}

export function useSystemEvents(): SystemEvent[] {
  const { client } = useCentrifugoContext();
  const [events, setEvents] = useState<SystemEvent[]>([]);

  useEffect(() => {
    if (!client) return;

    const channel = 'system:events';
    const existing = client.getSubscription(channel);
    if (existing) {
      existing.unsubscribe();
      existing.removeAllListeners();
      client.removeSubscription(existing);
    }

    const sub = client.newSubscription(channel);
    sub.on('publication', (ctx: PublicationContext) => {
      setEvents((prev) => [...prev, ctx.data as SystemEvent]);
    });
    sub.subscribe();

    return () => {
      sub.unsubscribe();
      sub.removeAllListeners();
      client.removeSubscription(sub);
    };
  }, [client]);

  return events;
}
