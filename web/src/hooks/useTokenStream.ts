import { useEffect, useState } from 'react';
import type { PublicationContext } from 'centrifuge';
import { useCentrifugoContext } from '@/hooks/useCentrifugo';

interface StreamTokenPayload {
  token: string;
  done: boolean;
  messageId: string;
}

export function useTokenStream(messageId: string | null): {
  text: string;
  isDone: boolean;
} {
  const { client } = useCentrifugoContext();
  const [text, setText] = useState('');
  const [isDone, setIsDone] = useState(false);

  useEffect(() => {
    if (!client || !messageId) {
      setText('');
      setIsDone(false);
      return;
    }

    setText('');
    setIsDone(false);
    const channel = `internal:stream:${messageId}`;

    const existing = client.getSubscription(channel);
    if (existing) {
      existing.unsubscribe();
      existing.removeAllListeners();
      client.removeSubscription(existing);
    }

    const sub = client.newSubscription(channel);
    sub.on('publication', (ctx: PublicationContext) => {
      const data = ctx.data as StreamTokenPayload;
      if (data.token) setText((prev) => prev + data.token);
      if (data.done) {
        setIsDone(true);
        sub.unsubscribe();
        sub.removeAllListeners();
        client.removeSubscription(sub);
      }
    });
    sub.subscribe();

    return () => {
      sub.unsubscribe();
      sub.removeAllListeners();
      try {
        client.removeSubscription(sub);
      } catch {
        // already removed
      }
    };
  }, [client, messageId]);

  return { text, isDone };
}
