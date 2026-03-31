/**
 * Shared types and errors for the tool execution system.
 */

export class PermissionDeniedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'PermissionDeniedError';
  }
}

export class NotPermittedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'NotPermittedError';
  }
}

/** Max output length in bytes (50 KB). */
export const MAX_RESULT_BYTES = 50_000;

/** Default timeout for shell commands in ms. */
export const DEFAULT_SHELL_TIMEOUT_MS = 30_000;

export const AGENT_ID = process.env['AGENT_INSTANCE_ID'] || process.env['AGENT_NAME'] || 'unknown';
