import { createContext } from 'react';
import type { Centrifuge } from 'centrifuge';

export type ConnectionState = 'connecting' | 'connected' | 'disconnected' | 'error';

export interface CentrifugoContextValue {
  client: Centrifuge | null;
  connectionState: ConnectionState;
}

export const CentrifugoContext = createContext<CentrifugoContextValue>({
  client: null,
  connectionState: 'disconnected',
});
