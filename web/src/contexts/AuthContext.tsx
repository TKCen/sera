import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { ApiError, getAuthMe, getToken, setToken, type AuthMe } from '@/lib/api';
import { AuthContext, type AuthStatus } from '@/contexts/auth-context';

export function AuthProvider({ children }: { children: ReactNode }) {
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
      }
      setIdentity(null);
      setStatus('unauthenticated');
    }
  }, []);

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
  }, []);

  return (
    <AuthContext.Provider value={{ status, identity, signIn, signOut }}>
      {children}
    </AuthContext.Provider>
  );
}
