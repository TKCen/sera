import { useEffect, useState } from 'react';
import type { PublicationContext } from 'centrifuge';
import { useCentrifugoContext } from '@/contexts/CentrifugoContext';
import type { ThoughtEvent } from '@/lib/api/types';

export function useThoughtStream(agentId: string): ThoughtEvent[] {
  const { client } = useCentrifugoContext();
  const [thoughts, setThoughts] = useState<ThoughtEvent[]>([]);

  useEffect(() => {
    if (!client || !agentId) return;

    setThoughts([]);
    const channel = `internal:agent:${agentId}:thoughts`;

    const existing = client.getSubscription(channel);
    if (existing) {
      existing.unsubscribe();
      existing.removeAllListeners();
      client.removeSubscription(existing);
    }

    const sub = client.newSubscription(channel);
    sub.on('publication', (ctx: PublicationContext) => {
      setThoughts((prev) => [...prev, ctx.data as ThoughtEvent]);
    });
    sub.subscribe();

    return () => {
      sub.unsubscribe();
      sub.removeAllListeners();
      client.removeSubscription(sub);
    };
  }, [client, agentId]);

  return thoughts;
}
