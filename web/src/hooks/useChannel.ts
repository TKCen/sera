import { useEffect, useRef, useState } from 'react';
import type { PublicationContext } from 'centrifuge';
import { useCentrifugoContext } from '@/contexts/CentrifugoContext';

export function useChannel<T = unknown>(channelName: string): T | null {
  const { client } = useCentrifugoContext();
  const [latestMessage, setLatestMessage] = useState<T | null>(null);
  const channelRef = useRef(channelName);
  channelRef.current = channelName;

  useEffect(() => {
    if (!client || !channelName) return;

    const existing = client.getSubscription(channelName);
    if (existing) {
      existing.unsubscribe();
      existing.removeAllListeners();
      client.removeSubscription(existing);
    }

    const sub = client.newSubscription(channelName);
    sub.on('publication', (ctx: PublicationContext) => {
      setLatestMessage(ctx.data as T);
    });
    sub.subscribe();

    return () => {
      sub.unsubscribe();
      sub.removeAllListeners();
      client.removeSubscription(sub);
    };
  }, [client, channelName]);

  return latestMessage;
}
