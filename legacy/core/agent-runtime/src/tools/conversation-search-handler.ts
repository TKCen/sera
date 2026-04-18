/**
 * Conversation search handler — lets agents search their own conversation history.
 *
 * Calls sera-core's GET /api/sessions/search endpoint, which queries the
 * chat_messages table filtered by the agent's instance ID.
 */

import { log } from '../logger.js';
import { isProxyAvailable } from './proxy.js';

const MAX_CONTENT_LENGTH = 500;
const DEFAULT_LIMIT = 10;
const MAX_LIMIT = 50;

export interface ConversationSearchParams {
  query: string;
  roles?: string[];
  start_date?: string;
  end_date?: string;
  limit?: number;
}

interface SearchResultMessage {
  id: string;
  sessionId: string;
  role: string;
  content: string;
  createdAt: string;
}

/**
 * Search the agent's own conversation history via sera-core.
 */
export async function conversationSearch(params: ConversationSearchParams): Promise<string> {
  if (!isProxyAvailable()) {
    return JSON.stringify({
      error: 'conversation_search is unavailable — SERA_CORE_URL not configured',
    });
  }

  const coreUrl = process.env['SERA_CORE_URL'] ?? '';
  const token = process.env['SERA_IDENTITY_TOKEN'] ?? '';
  const agentInstanceId = process.env['AGENT_INSTANCE_ID'];

  if (!agentInstanceId) {
    return JSON.stringify({
      error: 'conversation_search is unavailable — AGENT_INSTANCE_ID not set',
    });
  }

  const { query, roles, start_date, end_date, limit = DEFAULT_LIMIT } = params;

  if (!query || query.trim() === '') {
    return JSON.stringify({ error: 'query must not be empty' });
  }

  const clampedLimit = Math.min(Math.max(1, limit), MAX_LIMIT);

  const url = new URL(`${coreUrl}/api/sessions/search`);
  url.searchParams.set('agentInstanceId', agentInstanceId);
  url.searchParams.set('q', query);
  url.searchParams.set('limit', String(clampedLimit));

  if (roles && roles.length > 0) {
    url.searchParams.set('roles', roles.join(','));
  }
  if (start_date) {
    url.searchParams.set('startDate', start_date);
  }
  if (end_date) {
    url.searchParams.set('endDate', end_date);
  }

  try {
    const res = await fetch(url.toString(), {
      headers: { Authorization: `Bearer ${token}` },
      signal: AbortSignal.timeout(15_000),
    });

    if (!res.ok) {
      const body = await res.text();
      log('warn', `conversation_search: HTTP ${res.status}: ${body}`);
      return JSON.stringify({ error: `Search failed (HTTP ${res.status}): ${body}` });
    }

    const raw = (await res.json()) as SearchResultMessage[];

    const results = raw.map((msg) => ({
      id: msg.id,
      session_id: msg.sessionId,
      role: msg.role,
      timestamp: msg.createdAt,
      content:
        msg.content.length > MAX_CONTENT_LENGTH
          ? msg.content.substring(0, MAX_CONTENT_LENGTH) + '... [truncated]'
          : msg.content,
    }));

    return JSON.stringify({
      results,
      count: results.length,
      query,
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    log('warn', `conversation_search error: ${message}`);
    return JSON.stringify({ error: `Search request failed: ${message}` });
  }
}
