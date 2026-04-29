import { fetchEventSource } from '@microsoft/fetch-event-source';

const API_BASE = '/api';
const AUTH_KEY = 'sera.auth.token';

export function getToken(): string | null {
  return localStorage.getItem(AUTH_KEY);
}

export function setToken(token: string | null): void {
  if (token) {
    localStorage.setItem(AUTH_KEY, token);
  } else {
    localStorage.removeItem(AUTH_KEY);
  }
}

export class ApiError extends Error {
  constructor(
    public status: number,
    public body: unknown,
    message: string,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

export async function apiFetch<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  const token = getToken();
  if (token) headers.set('Authorization', `Bearer ${token}`);
  if (shouldAttachJsonContentType(init.body, headers)) {
    headers.set('Content-Type', 'application/json');
  }

  const res = await fetch(`${API_BASE}${path}`, { ...init, headers });
  const text = await res.text();
  const body = text ? safeJson(text) : null;

  if (!res.ok) {
    const msg =
      typeof body === 'object' && body && 'error' in body
        ? String((body as { error: unknown }).error)
        : res.statusText;
    throw new ApiError(res.status, body, msg);
  }

  return body as T;
}

function shouldAttachJsonContentType(body: BodyInit | null | undefined, headers: Headers): boolean {
  if (!body || headers.has('Content-Type')) return false;
  // Browser-managed bodies set their own Content-Type (with multipart boundary etc.).
  // Only auto-set application/json for plain string payloads.
  return typeof body === 'string';
}

function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

// ── Typed endpoints ────────────────────────────────────────────────────────

export interface Health {
  status: 'ok';
}

export interface Readiness {
  status: 'ready' | 'not_ready';
  runtime_connected: boolean;
}

export interface AuthMe {
  id: string;
  principal_id: string;
  sub: string;
  roles: string[];
  mode: string;
}

export const getHealth = () => apiFetch<Health>('/health');

export const getReadiness = () =>
  apiFetch<Readiness>('/health/ready').catch((err) => {
    // 503 carries the same {status, runtime_connected} shape on the not-ready path.
    if (err instanceof ApiError && err.status === 503 && isReadiness(err.body)) {
      return err.body;
    }
    throw err;
  });

function isReadiness(value: unknown): value is Readiness {
  if (typeof value !== 'object' || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    (v.status === 'ready' || v.status === 'not_ready') &&
    typeof v.runtime_connected === 'boolean'
  );
}

export const getAuthMe = () => apiFetch<AuthMe>('/auth/me');

// ── Agents ─────────────────────────────────────────────────────────────────

export interface AgentInfo {
  name: string;
  provider: string;
  model?: string | null;
  has_tools: boolean;
}

// Detail shape is loose JSON from the gateway — use a wide type.
export type AgentDetail = Record<string, unknown> & { name?: string; provider?: string };

export const getAgents = () => apiFetch<AgentInfo[]>('/agents');
export const getAgent = (id: string) => apiFetch<AgentDetail>(`/agents/${encodeURIComponent(id)}`);

// ── Chat streaming ─────────────────────────────────────────────────────────

export interface ChatStreamDelta {
  type: 'delta';
  delta: string;
  messageId: string;
  sessionId: string;
}

export interface ChatStreamDone {
  type: 'done';
  usage: { promptTokens: number; completionTokens: number; totalTokens: number };
}

export type ChatStreamEvent = ChatStreamDelta | ChatStreamDone;

/** Parse a raw SSE message into a ChatStreamEvent. Returns null for unknown event types or malformed JSON. */
export function parseChatSseEvent(
  eventType: string,
  dataJson: string,
): ChatStreamEvent | null {
  try {
    if (eventType === 'message') {
      const raw = JSON.parse(dataJson) as {
        delta: string;
        message_id: string;
        session_id: string;
      };
      return {
        type: 'delta',
        delta: raw.delta,
        messageId: raw.message_id,
        sessionId: raw.session_id,
      };
    }
    if (eventType === 'done') {
      const raw = JSON.parse(dataJson) as {
        status: string;
        usage?: { prompt_tokens: number; completion_tokens: number; total_tokens: number };
      };
      return {
        type: 'done',
        usage: {
          promptTokens: raw.usage?.prompt_tokens ?? 0,
          completionTokens: raw.usage?.completion_tokens ?? 0,
          totalTokens: raw.usage?.total_tokens ?? 0,
        },
      };
    }
  } catch {
    // Malformed JSON — ignore gracefully, same contract as unknown event types
    return null;
  }
  // Unknown event type — ignore gracefully
  return null;
}

export interface ChatStreamOptions {
  message: string;
  signal: AbortSignal;
  onEvent: (event: ChatStreamEvent) => void;
  onError: (err: unknown) => void;
}

export async function chatStream({
  message,
  signal,
  onEvent,
  onError,
}: ChatStreamOptions): Promise<void> {
  const token = getToken();
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
  };
  if (token) headers['Authorization'] = `Bearer ${token}`;

  await fetchEventSource(`${API_BASE}/chat`, {
    method: 'POST',
    headers,
    body: JSON.stringify({ message, stream: true }),
    signal,
    async onopen(response) {
      if (!response.ok) {
        const text = await response.text();
        const body = text ? safeJson(text) : null;
        const msg =
          typeof body === 'object' && body && 'error' in body
            ? String((body as { error: unknown }).error)
            : response.statusText;
        throw new ApiError(response.status, body, msg);
      }
    },
    onmessage(ev) {
      const parsed = parseChatSseEvent(ev.event ?? '', ev.data);
      if (parsed) onEvent(parsed);
    },
    onerror(err) {
      onError(err);
      // Returning undefined lets fetchEventSource retry by default — throw to stop
      throw err;
    },
  });
}

// ── Sessions ───────────────────────────────────────────────────────────────

export interface SessionInfo {
  id: string;
  agent_id: string;
  session_key: string;
  state: string;
  principal_id: string | null;
  created_at: string;
  updated_at: string | null;
}

export interface TranscriptEntry {
  id: number;
  session_id: string;
  role: string;
  content: string | null;
  tool_calls: string | null;
  tool_call_id: string | null;
  created_at: string;
}

export const getSessions = () => apiFetch<SessionInfo[]>('/sessions');

export const getSessionTranscript = (id: string) =>
  apiFetch<TranscriptEntry[]>(`/sessions/${encodeURIComponent(id)}/transcript`);
