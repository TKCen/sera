/**
 * Tool definitions — OpenAI function-calling schemas for the LLM.
 *
 * These are the LOCAL tools that the agent-runtime can execute natively.
 * In the future, remote tools will be fetched from core's catalog endpoint
 * (GET /v1/llm/tools) per ADR-001.
 */

import type { ToolDefinition } from '../llmClient.js';

export const BUILTIN_TOOLS: ToolDefinition[] = [
  {
    type: 'function',
    function: {
      name: 'file-read',
      description: 'Read the contents of a file from the workspace.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Relative path to the file within the workspace.',
          },
        },
        required: ['path'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'read_file',
      description: 'Read a file with optional offset and limit for large files.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Relative path to the file within the workspace.',
          },
          offset: {
            type: 'number',
            description: 'Starting line number (1-indexed). Defaults to 1.',
          },
          limit: {
            type: 'number',
            description: 'Maximum number of lines to read. Defaults to 500.',
          },
        },
        required: ['path'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'glob',
      description:
        'Find files using a glob pattern. Returns sorted paths, respects .gitignore, max 1000 files.',
      parameters: {
        type: 'object',
        properties: {
          pattern: {
            type: 'string',
            description: 'Glob pattern to search for (e.g., "**/*.ts").',
          },
        },
        required: ['pattern'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'grep',
      description: 'Search for content in files using ripgrep.',
      parameters: {
        type: 'object',
        properties: {
          pattern: {
            type: 'string',
            description: 'Regex pattern to search for.',
          },
          path: {
            type: 'string',
            description: 'Relative path to directory or file to search in. Defaults to root.',
          },
          mode: {
            type: 'string',
            enum: ['files_with_matches', 'content', 'count'],
            description:
              'Search mode: files_with_matches (list files), content (show matches with line numbers), count (count matches per file).',
          },
        },
        required: ['pattern'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'file-write',
      description: 'Write content to a file in the workspace. Creates parent directories if needed.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Relative path to the file within the workspace.',
          },
          content: {
            type: 'string',
            description: 'Content to write to the file.',
          },
        },
        required: ['path', 'content'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'file-list',
      description: 'List directory contents with type (file/dir) and size.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Relative path to the directory within the workspace. Defaults to root.',
          },
        },
        required: [],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'file-delete',
      description: 'Delete a file or empty directory in the workspace.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Relative path to the file or directory within the workspace.',
          },
          recursive: {
            type: 'boolean',
            description: 'If true, delete a non-empty directory and all its contents.',
          },
        },
        required: ['path'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'shell-exec',
      description: 'Execute a shell command in the workspace directory. Returns stdout/stderr/exitCode.',
      parameters: {
        type: 'object',
        properties: {
          command: {
            type: 'string',
            description: 'The shell command to execute.',
          },
          timeout_ms: {
            type: 'number',
            description: 'Command timeout in milliseconds. Defaults to 30000.',
          },
        },
        required: ['command'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'spawn-subagent',
      description:
        'Spawn a child agent to handle a delegated task. The subagent runs in its own container ' +
        "and returns the result when complete. Requires the role to be in the parent manifest's allowed subagents.",
      parameters: {
        type: 'object',
        properties: {
          role: {
            type: 'string',
            description: "The subagent role to spawn (must be in the manifest's subagents.allowed list).",
          },
          task: {
            type: 'string',
            description: 'Task description for the subagent to execute.',
          },
        },
        required: ['role', 'task'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'run-tool',
      description:
        'Run an ephemeral tool in a short-lived container. The container executes the command ' +
        'and returns stdout/stderr. Useful for running tools that need isolation.',
      parameters: {
        type: 'object',
        properties: {
          tool_name: {
            type: 'string',
            description: 'Name of the tool to run.',
          },
          command: {
            type: 'string',
            description: 'Shell command to execute inside the tool container.',
          },
          timeout_seconds: {
            type: 'number',
            description: 'Timeout in seconds (60-300). Defaults to 120.',
          },
        },
        required: ['tool_name', 'command'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'tool-search',
      description:
        'Search for additional tools by capability description. ' +
        'Use this when you need a tool that is not in your current set. ' +
        'Returns matching tool names and descriptions.',
      parameters: {
        type: 'object',
        properties: {
          query: {
            type: 'string',
            description: 'Description of the capability you need (e.g., "schedule a task", "search the web").',
          },
        },
        required: ['query'],
      },
    },
  },
];
