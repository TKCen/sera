/// <reference types="vite/client" />
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Centrifuge } from 'centrifuge';
import { getRtToken } from '@/lib/api/rt';
import { CentrifugoContext, type ConnectionState } from './CentrifugoContext.types';

const REFRESH_ADVANCE_MS = 60_000;

function getCentrifugoUrl(): string {
  const envUrl = import.meta.env.VITE_CENTRIFUGO_URL;
  if (envUrl) return envUrl;
  const proto = window.location.protocol === 'https:' ? 'wss' : 'ws';
  return `${proto}://${window.location.hostname}:10001/connection/websocket`;
}

export function CentrifugoProvider({ children }: { children: ReactNode }) {
  const [connectionState, setConnectionState] = useState<ConnectionState>('connecting');
  const clientRef = useRef<Centrifuge | null>(null);
  const tokenTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    let destroyed = false;

    async function getToken(): Promise<string> {
      const { token, expiresAt } = await getRtToken();

      if (!destroyed && tokenTimerRef.current === null) {
        const msUntilRefresh = Math.max(0, expiresAt * 1000 - Date.now() - REFRESH_ADVANCE_MS);
        tokenTimerRef.current = setTimeout(() => {
          tokenTimerRef.current = null;
          if (!destroyed && clientRef.current) {
            void clientRef.current.setToken('');
          }
        }, msUntilRefresh);
      }

      return token;
    }

    const client = new Centrifuge(getCentrifugoUrl(), {
      getToken: async () => {
        try {
          return await getToken();
        } catch {
          return '';
        }
      },
    });

    client.on('connecting', () => !destroyed && setConnectionState('connecting'));
    client.on('connected', () => !destroyed && setConnectionState('connected'));
    client.on('disconnected', () => !destroyed && setConnectionState('disconnected'));
    client.on('error', () => !destroyed && setConnectionState('error'));

    clientRef.current = client;
    client.connect();

    return () => {
      destroyed = true;
      if (tokenTimerRef.current) {
        clearTimeout(tokenTimerRef.current);
        tokenTimerRef.current = null;
      }
      client.disconnect();
      clientRef.current = null;
    };
  }, []);

  return (
    <CentrifugoContext.Provider value={{ client: clientRef.current, connectionState }}>
      {children}
    </CentrifugoContext.Provider>
  );
}
