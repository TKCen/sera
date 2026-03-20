import { useContext } from 'react';
import {
  AuthContext,
  AuthInternalContext,
  type AuthContextValue,
  type AuthInternalContextValue,
} from '@/contexts/AuthContext.types';

export function useAuth(): AuthContextValue {
  return useContext(AuthContext);
}

export function useAuthInternal(): AuthInternalContextValue {
  return useContext(AuthInternalContext);
}
