import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: http-request
 *
 * Full HTTP client for API interaction (beyond web-fetch which is for web pages).
 * Supports methods, headers, and request body.
 */
export const httpRequestSkill: SkillDefinition = {
  id: 'http-request',
  description: 'Full HTTP client for API interaction. Supports GET, POST, PUT, PATCH, DELETE.',
  source: 'builtin',
  parameters: [
    {
      name: 'url',
      type: 'string',
      description: 'The URL to fetch',
      required: true,
    },
    {
      name: 'method',
      type: 'string',
      description: 'The HTTP method (default: GET)',
      required: false,
    },
    {
      name: 'headers',
      type: 'object',
      description: 'Optional HTTP headers',
      required: false,
    },
    {
      name: 'body',
      type: 'string',
      description: 'Optional HTTP request body',
      required: false,
    },
    {
      name: 'timeout',
      type: 'number',
      description: 'Timeout in milliseconds (default: 30000)',
      required: false,
    },
  ],
  handler: async () => {
    // This handler is a stub; the actual execution happens in the agent-runtime.
    return { success: true, data: 'HTTP request complete.' };
  },
};
