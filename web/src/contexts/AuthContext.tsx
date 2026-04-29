import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { ApiError, getAuthMe, getToken, setToken, type AuthMe } from '@/lib/api';
import { AuthContext, type AuthStatus } from '@/contexts/auth-context';

export function AuthProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient();
  const [status, setStatus] = useState<AuthStatus>('loading');
  const [identity, setIdentity] = useState<AuthMe | null>(null);

  const validate = useCallback(async () => {
    if (!getToken()) {
      setStatus('unauthenticated');
      setIdentity(null);
      return;
    }
    try {
      const me = await getAuthMe();
      setIdentity(me);
      setStatus('authenticated');
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) {
        setToken(null);
        queryClient.clear();
      }
      setIdentity(null);
      setStatus('unauthenticated');
    }
  }, [queryClient]);

  useEffect(() => {
    void validate();
  }, [validate]);

  const signIn = useCallback(async (token: string) => {
    setToken(token);
    setStatus('loading');
    try {
      const me = await getAuthMe();
      setIdentity(me);
      setStatus('authenticated');
    } catch (err) {
      setToken(null);
      setStatus('unauthenticated');
      throw err;
    }
  }, []);

  const signOut = useCallback(() => {
    setToken(null);
    setIdentity(null);
    setStatus('unauthenticated');
    queryClient.clear();
  }, [queryClient]);

  return (
    <AuthContext.Provider value={{ status, identity, signIn, signOut }}>
      {children}
    </AuthContext.Provider>
  );
}
