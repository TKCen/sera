#!/usr/bin/env node
/**
 * Standalone MCP stdio server for Claude Desktop / Claude Code.
 *
 * Usage in claude_desktop_config.json:
 * {
 *   "mcpServers": {
 *     "sera": {
 *       "command": "bun",
 *       "args": ["run", "D:/projects/homelab/sera/core/src/mcp/stdio-server.ts"],
 *       "env": {
 *         "SERA_API_URL": "http://localhost:3000",
 *         "SERA_API_KEY": "sera_bootstrap_dev_123"
 *       }
 *     }
 *   }
 * }
 */

import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { CallToolRequestSchema, ListToolsRequestSchema } from '@modelcontextprotocol/sdk/types.js';

const SERA_API_URL = process.env.SERA_API_URL ?? 'http://localhost:3000';
const SERA_API_KEY = process.env.SERA_API_KEY ?? '';

function headers(): Record<string, string> {
  const h: Record<string, string> = { 'Content-Type': 'application/json' };
  if (SERA_API_KEY) h['Authorization'] = `Bearer ${SERA_API_KEY}`;
  return h;
}

async function seraFetch(path: string, init?: RequestInit): Promise<unknown> {
  const res = await fetch(`${SERA_API_URL}/api${path}`, {
    ...init,
    headers: { ...headers(), ...(init?.headers as Record<string, string>) },
    signal: AbortSignal.timeout(600_000),
  });
  if (!res.ok) {
    const body = await res.text().catch(() => '');
    throw new Error(`SERA API ${res.status}: ${body}`);
  }
  return res.json();
}

const server = new Server({ name: 'sera', version: '1.0.0' }, { capabilities: { tools: {} } });

server.setRequestHandler(ListToolsRequestSchema, async () => ({
  tools: [
    {
      name: 'sera_chat',
      description:
        'Send a message to a SERA agent and get a structured response with reasoning steps. ' +
        'Pass sessionId to continue a multi-turn conversation. ' +
        'Returns: session ID, reasoning steps (tool calls, thoughts), reply text, and citations.',
      inputSchema: {
        type: 'object' as const,
        properties: {
          agentName: { type: 'string', description: 'Agent name (e.g. "sera")' },
          message: { type: 'string', description: 'Message or task for the agent' },
          sessionId: {
            type: 'string',
            description: 'Session ID from a previous sera_chat call to continue the conversation',
          },
        },
        required: ['agentName', 'message'],
      },
    },
    {
      name: 'sera_list_agents',
      description: 'List all SERA agent instances with their status.',
      inputSchema: { type: 'object' as const, properties: {} },
    },
    {
      name: 'sera_agent_status',
      description: 'Get detailed info for a specific agent.',
      inputSchema: {
        type: 'object' as const,
        properties: {
          agentId: { type: 'string', description: 'Agent instance ID' },
        },
        required: ['agentId'],
      },
    },
    {
      name: 'sera_knowledge_query',
      description: "Semantic search across an agent's knowledge using RAG.",
      inputSchema: {
        type: 'object' as const,
        properties: {
          agentId: { type: 'string', description: 'Agent instance ID' },
          query: { type: 'string', description: 'Search query' },
        },
        required: ['agentId', 'query'],
      },
    },
    {
      name: 'sera_knowledge_store',
      description: 'Store a knowledge entry for an agent.',
      inputSchema: {
        type: 'object' as const,
        properties: {
          agentId: { type: 'string', description: 'Agent instance ID' },
          content: { type: 'string', description: 'Content to store' },
          type: {
            type: 'string',
            description:
              'Block type: fact, context, memory, insight, reference, observation, decision',
          },
          title: { type: 'string', description: 'Title' },
          tags: { type: 'string', description: 'Comma-separated tags' },
        },
        required: ['agentId', 'content', 'type'],
      },
    },
    {
      name: 'sera_list_sessions',
      description: 'List chat sessions for an agent.',
      inputSchema: {
        type: 'object' as const,
        properties: {
          agentId: { type: 'string', description: 'Agent instance ID' },
        },
        required: ['agentId'],
      },
    },
    {
      name: 'sera_memory_blocks',
      description: "List an agent's memory blocks with optional tag filtering.",
      inputSchema: {
        type: 'object' as const,
        properties: {
          agentId: { type: 'string', description: 'Agent instance ID' },
          tags: { type: 'string', description: 'Comma-separated tags to filter by' },
          type: { type: 'string', description: 'Block type filter' },
        },
        required: ['agentId'],
      },
    },
    {
      name: 'sera_start_agent',
      description: 'Start an agent instance.',
      inputSchema: {
        type: 'object' as const,
        properties: {
          agentId: { type: 'string', description: 'Agent instance ID' },
        },
        required: ['agentId'],
      },
    },
    {
      name: 'sera_stop_agent',
      description: 'Stop an agent instance.',
      inputSchema: {
        type: 'object' as const,
        properties: {
          agentId: { type: 'string', description: 'Agent instance ID' },
        },
        required: ['agentId'],
      },
    },
    {
      name: 'sera_operator_requests',
      description:
        'List pending operator requests from SERA agents. Agents create these when they need operator help (config changes, dependency installs, capability requests).',
      inputSchema: {
        type: 'object' as const,
        properties: {
          status: {
            type: 'string',
            description:
              'Filter by status: pending, approved, rejected, resolved (default: pending)',
          },
        },
      },
    },
    {
      name: 'sera_operator_respond',
      description: 'Respond to an operator request from a SERA agent.',
      inputSchema: {
        type: 'object' as const,
        properties: {
          requestId: { type: 'string', description: 'The operator request ID' },
          action: {
            type: 'string',
            description: 'Response action: approved, rejected, or resolved',
          },
          response: {
            type: 'string',
            description: 'Optional response message or JSON data for the agent',
          },
        },
        required: ['requestId', 'action'],
      },
    },
    {
      name: 'sera_mcp_servers',
      description: 'List all registered MCP servers with status and tool counts.',
      inputSchema: { type: 'object' as const, properties: {} },
    },
  ],
}));

server.setRequestHandler(CallToolRequestSchema, async (request) => {
  const { name, arguments: args } = request.params;
  const a = (args ?? {}) as Record<string, unknown>;

  try {
    switch (name) {
      case 'sera_chat': {
        const body: Record<string, unknown> = {
          agentName: a.agentName as string,
          message: a.message as string,
        };
        if (a.sessionId) body.sessionId = a.sessionId as string;
        const result = await seraFetch('/chat', {
          method: 'POST',
          body: JSON.stringify(body),
        });
        const r = result as {
          sessionId: string;
          reply: string;
          thought?: string;
          thoughts?: Array<{ type: string; content: string }>;
          citations?: Array<{ url: string; title?: string }>;
        };
        // Structured response for agent consumption
        const parts: string[] = [];
        parts.push(`**Session:** ${r.sessionId}`);
        if (r.thoughts?.length) {
          parts.push('\n**Reasoning steps:**');
          for (const t of r.thoughts) {
            parts.push(`- [${t.type}] ${t.content}`);
          }
        }
        parts.push(`\n**Reply:**\n${r.reply}`);
        if (r.citations?.length) {
          parts.push('\n**Citations:**');
          for (const c of r.citations) {
            parts.push(`- ${c.title ?? c.url}: ${c.url}`);
          }
        }
        return { content: [{ type: 'text', text: parts.join('\n') }] };
      }

      case 'sera_list_agents': {
        const agents = await seraFetch('/agents');
        return { content: [{ type: 'text', text: JSON.stringify(agents, null, 2) }] };
      }

      case 'sera_agent_status': {
        const agent = await seraFetch(
          `/agents/instances/${encodeURIComponent(a.agentId as string)}`
        );
        return { content: [{ type: 'text', text: JSON.stringify(agent, null, 2) }] };
      }

      case 'sera_knowledge_query': {
        // Use the memory blocks endpoint with semantic search not yet exposed via REST,
        // so fall back to listing blocks for now
        const blocks = await seraFetch(`/memory/${encodeURIComponent(a.agentId as string)}/blocks`);
        return { content: [{ type: 'text', text: JSON.stringify(blocks, null, 2) }] };
      }

      case 'sera_knowledge_store': {
        const tags =
          typeof a.tags === 'string' ? (a.tags as string).split(',').map((t) => t.trim()) : [];
        const blockType = encodeURIComponent(a.type as string);
        const body = {
          title: (a.title as string) ?? 'Untitled',
          content: a.content,
          ...(tags.length > 0 ? { tags } : {}),
          source: 'mcp',
        };
        // Use the legacy blocks endpoint: POST /api/memory/blocks/:type
        const result = await seraFetch(`/memory/blocks/${blockType}`, {
          method: 'POST',
          body: JSON.stringify(body),
        });
        return { content: [{ type: 'text', text: JSON.stringify(result, null, 2) }] };
      }

      case 'sera_list_sessions': {
        const sessions = await seraFetch(
          `/sessions?agentInstanceId=${encodeURIComponent(a.agentId as string)}`
        );
        return { content: [{ type: 'text', text: JSON.stringify(sessions, null, 2) }] };
      }

      case 'sera_memory_blocks': {
        const params = new URLSearchParams();
        if (a.tags) params.set('tags', a.tags as string);
        if (a.type) params.set('type', a.type as string);
        const qs = params.toString();
        const blocks = await seraFetch(
          `/memory/${encodeURIComponent(a.agentId as string)}/blocks${qs ? `?${qs}` : ''}`
        );
        return { content: [{ type: 'text', text: JSON.stringify(blocks, null, 2) }] };
      }

      case 'sera_start_agent': {
        const result = await seraFetch(
          `/agents/instances/${encodeURIComponent(a.agentId as string)}/start`,
          {
            method: 'POST',
          }
        );
        return { content: [{ type: 'text', text: JSON.stringify(result, null, 2) }] };
      }

      case 'sera_stop_agent': {
        const result = await seraFetch(
          `/agents/instances/${encodeURIComponent(a.agentId as string)}/stop`,
          {
            method: 'POST',
          }
        );
        return { content: [{ type: 'text', text: JSON.stringify(result, null, 2) }] };
      }

      case 'sera_operator_requests': {
        const status = (a.status as string) || 'pending';
        const requests = await seraFetch(`/operator-requests?status=${encodeURIComponent(status)}`);
        return { content: [{ type: 'text', text: JSON.stringify(requests, null, 2) }] };
      }

      case 'sera_operator_respond': {
        const result = await seraFetch(
          `/operator-requests/${encodeURIComponent(a.requestId as string)}/respond`,
          {
            method: 'POST',
            body: JSON.stringify({
              action: a.action,
              ...(a.response ? { response: a.response } : {}),
            }),
          }
        );
        return { content: [{ type: 'text', text: JSON.stringify(result, null, 2) }] };
      }

      case 'sera_mcp_servers': {
        const servers = await seraFetch('/mcp-servers');
        return { content: [{ type: 'text', text: JSON.stringify(servers, null, 2) }] };
      }

      default:
        throw new Error(`Unknown tool: ${name}`);
    }
  } catch (err) {
    return {
      isError: true,
      content: [
        { type: 'text', text: `Error: ${err instanceof Error ? err.message : String(err)}` },
      ],
    };
  }
});

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

main().catch((err) => {
  process.stderr.write(`SERA MCP server error: ${err}\n`);
  process.exit(1);
});
