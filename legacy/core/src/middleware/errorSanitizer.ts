import type { Request, Response, NextFunction } from 'express';
import { Logger } from '../lib/logger.js';

const logger = new Logger('ErrorHandler');

/**
 * Patterns that indicate internal details that should not be exposed to API consumers.
 * If an error message matches any of these, we replace it with a generic message.
 */
const SENSITIVE_PATTERNS = [
  /\/app\//i, // Container paths
  /\/home\//i, // Host paths
  /node_modules/i, // Dependency paths
  /at\s+\w+\s+\(/i, // Stack trace lines
  /ECONNREFUSED/i, // Internal connection errors
  /ENOTFOUND/i, // DNS resolution failures
  /password|secret|key|token/i, // Credential references (but not "API key" in user-facing messages)
  /SELECT|INSERT|UPDATE|DELETE|FROM\s+\w+/i, // SQL fragments
  /pg_/i, // PostgreSQL internals
  /FATAL:/i, // Database fatal errors
];

/** User-facing error messages that are safe to pass through. */
const SAFE_PREFIXES = [
  'Block ', // CoreMemoryService errors
  'Agent ', // Agent not found etc.
  'Model ', // Model not found
  'No provider', // Provider errors
  'Budget ', // Budget errors
  'Invalid ', // Validation errors
  'Missing ', // Missing fields
  'Authentication ', // Auth errors
  'Access denied', // Permission errors
  'Replace failed', // Core memory replace
  'Append failed', // Core memory append
  'Not found', // Generic not found
];

/**
 * Sanitize an error message for external consumption.
 * Returns the original message if it appears safe, or a generic message if it contains internals.
 */
export function sanitizeErrorMessage(message: string): string {
  // Allow known-safe user-facing messages through
  if (SAFE_PREFIXES.some((prefix) => message.startsWith(prefix))) {
    return message;
  }

  // Block messages that match sensitive patterns
  if (SENSITIVE_PATTERNS.some((pattern) => pattern.test(message))) {
    return 'An internal error occurred';
  }

  // Short messages (< 200 chars) without sensitive patterns are likely user-facing
  if (message.length < 200) {
    return message;
  }

  // Long messages are likely stack traces or verbose internals
  return 'An internal error occurred';
}

/**
 * Express error-handling middleware.
 * Catches unhandled errors thrown from route handlers and returns sanitized responses.
 * Must be registered AFTER all routes.
 */
export function errorSanitizerMiddleware(
  err: Error,
  req: Request,
  res: Response,
  _next: NextFunction
): void {
  const status =
    (err as Error & { status?: number; statusCode?: number }).status ??
    (err as Error & { status?: number; statusCode?: number }).statusCode ??
    500;

  // Always log the full error internally
  logger.error(`Unhandled error on ${req.method} ${req.path}:`, err.message);

  if (res.headersSent) {
    return;
  }

  res.status(status).json({
    error: sanitizeErrorMessage(err.message),
  });
}
