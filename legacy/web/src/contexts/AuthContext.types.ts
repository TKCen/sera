import { createContext } from 'react';

export interface User {
  sub: string;
  name?: string;
  email?: string;
  roles?: string[];
}

export interface AuthContextValue {
  isAuthenticated: boolean;
  user: User | null;
  roles: string[];
  isLoading: boolean;
  login: () => void;
  logout: () => void;
}

export const AuthContext = createContext<AuthContextValue>({
  isAuthenticated: false,
  user: null,
  roles: [],
  isLoading: true,
  login: () => undefined,
  logout: () => undefined,
});

export interface AuthInternalContextValue {
  setSessionAndUser: (token: string, user: User) => void;
  getPKCEVerifier: () => string | null;
  clearPKCEVerifier: () => void;
}

export const AuthInternalContext = createContext<AuthInternalContextValue>({
  setSessionAndUser: () => undefined,
  getPKCEVerifier: () => null,
  clearPKCEVerifier: () => undefined,
});
