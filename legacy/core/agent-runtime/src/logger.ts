/**
 * Simple logger for the agent runtime.
 * Outputs structured log lines to stdout.
 */

const AGENT_NAME = process.env.AGENT_NAME || 'agent-runtime';

export type LogLevel = 'debug' | 'info' | 'warn' | 'error';

export function log(level: LogLevel, message: string, data?: unknown): void {
  const timestamp = new Date().toISOString();
  const prefix = `[${timestamp}] [${AGENT_NAME}] [${level.toUpperCase()}]`;

  if (data !== undefined) {
    console.log(`${prefix} ${message}`, data);
  } else {
    console.log(`${prefix} ${message}`);
  }
}
