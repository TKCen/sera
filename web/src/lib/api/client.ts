/// <reference types="vite/client" />
import type { ErrorResponse } from './types';

const API_BASE_URL = import.meta.env.DEV ? '' : (import.meta.env.VITE_API_URL ?? '');

type AuthHeaderGetter = () => string | null;

let _getAuthHeader: AuthHeaderGetter = () => {
  const devKey = import.meta.env.VITE_DEV_API_KEY;
  if (devKey) return `Bearer ${devKey}`;
  return null;
};

export function setAuthHeaderGetter(fn: AuthHeaderGetter): void {
  _getAuthHeader = fn;
}

export class APIError extends Error {
  constructor(
    public readonly status: number,
    message: string,
    public readonly code?: string
  ) {
    super(message);
    this.name = 'APIError';
  }
}

const unauthorizedListeners: Array<() => void> = [];

export function onUnauthorized(fn: () => void): () => void {
  unauthorizedListeners.push(fn);
  return () => {
    const i = unauthorizedListeners.indexOf(fn);
    if (i !== -1) unauthorizedListeners.splice(i, 1);
  };
}

function notifyUnauthorized(): void {
  unauthorizedListeners.forEach((fn) => fn());
}

export async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const authHeader = _getAuthHeader();
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string>),
  };
  if (authHeader) headers['Authorization'] = authHeader;

  const url = `${API_BASE_URL}/api${path}`;
  const response = await fetch(url, { ...options, headers });

  if (response.status === 401) {
    notifyUnauthorized();
    throw new APIError(401, 'Unauthorized');
  }

  if (!response.ok) {
    let message = response.statusText;
    let code: string | undefined;
    try {
      const body = (await response.json()) as ErrorResponse;
      message = body.error ?? message;
      code = body.code;
    } catch {
      // non-JSON error body, keep statusText
    }
    throw new APIError(response.status, message, code);
  }

  if (response.status === 204) return undefined as T;

  const body = await response.json() as T;

  // Guard: backend may return 200 with { error: "..." } (e.g. via dev proxy)
  if (body && typeof body === 'object' && 'error' in body && !('id' in body)) {
    const errBody = body as unknown as ErrorResponse;
    if (errBody.error) {
      throw new APIError(response.status, errBody.error, errBody.code);
    }
  }

  return body;
}

export async function requestText(path: string, options: RequestInit = {}): Promise<string> {
  const authHeader = _getAuthHeader();
  const headers: Record<string, string> = {
    ...(options.headers as Record<string, string>),
  };
  if (authHeader) headers['Authorization'] = authHeader;

  const url = `${API_BASE_URL}/api${path}`;
  const response = await fetch(url, { ...options, headers });

  if (response.status === 401) {
    notifyUnauthorized();
    throw new APIError(401, 'Unauthorized');
  }

  if (!response.ok) {
    throw new APIError(response.status, response.statusText);
  }

  return response.text();
}
