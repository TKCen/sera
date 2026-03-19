import { useCentrifugoContext, type ConnectionState } from '@/contexts/CentrifugoContext';

export function useCentrifugo(): { connectionState: ConnectionState } {
  const { connectionState } = useCentrifugoContext();
  return { connectionState };
}
