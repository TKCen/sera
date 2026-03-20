import { useContext } from 'react';
import { CentrifugoContext, type CentrifugoContextValue } from '@/contexts/CentrifugoContext.types';

export function useCentrifugoContext(): CentrifugoContextValue {
  return useContext(CentrifugoContext);
}
