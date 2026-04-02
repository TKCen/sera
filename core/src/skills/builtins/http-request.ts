import type { SkillDefinition } from '../types.js';

/**
 * Built-in skill: http-request
 *
 * Full HTTP client for API interaction.
 */
export const httpRequestSkill: SkillDefinition = {
  id: 'http-request',
  description:
    'Full HTTP client for API interaction (raw HTTP with method, headers, and body).',
  source: 'builtin',
  parameters: [
    {
      name: 'url',
      type: 'string',
      description: 'The URL to request',
      required: true,
    },
    {
      name: 'method',
      type: 'string',
      description: 'HTTP method (GET, POST, PUT, DELETE, PATCH). Default: GET',
      required: false,
    },
    {
      name: 'headers',
      type: 'object',
      description: 'Optional request headers',
      required: false,
    },
    {
      name: 'body',
      type: 'string',
      description: 'Optional request body (JSON string)',
      required: false,
    },
    {
      name: 'timeout',
      type: 'number',
      description: 'Timeout in ms (default: 30000)',
      required: false,
    },
  ],
  handler: async (params, context) => {
    // Execution is handled by the agent-runtime
    return {
      success: true,
      data: {
        url: params['url'],
        method: params['method'],
        headers: params['headers'],
        body: params['body'],
        timeout: params['timeout'],
      },
    };
  },
};
