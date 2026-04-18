/**
 * Tool argument repair — fixes common LLM JSON malformations before parsing.
 *
 * LLMs frequently produce slightly malformed JSON in tool call arguments:
 * trailing commas, single quotes, unquoted keys, truncated output, etc.
 * This module attempts progressive repair before falling back to JSON5.
 */

import JSON5 from 'json5';

// ── Types ────────────────────────────────────────────────────────────────────

export type RepairStrategy =
  | 'markdown-code-block'
  | 'trailing-commas'
  | 'single-quotes'
  | 'unquoted-keys'
  | 'truncated'
  | 'comments'
  | 'newlines'
  | 'json5';

export interface RepairResult {
  parsed: Record<string, unknown>;
  repaired: boolean;
  strategy: RepairStrategy | null;
}

// ── Repair strategies ────────────────────────────────────────────────────────

/** Strip markdown code block wrappers (```json ... ```) */
function stripMarkdownCodeBlocks(s: string): string {
  const match = s.match(/^```(?:json)?\s*([\s\S]*?)\s*```$/m)
             || s.match(/```(?:json)?\s*([\s\S]*?)\s*```/);
  return match?.[1]?.trim() ?? s;
}

/** Strip trailing commas before } or ] */
function stripTrailingCommas(s: string): string {
  return s.replace(/,\s*([}\]])/g, '$1');
}

/**
 * Replace single-quoted strings with double-quoted strings.
 * Uses a state machine to avoid replacing apostrophes inside double-quoted strings.
 */
function fixSingleQuotes(s: string): string {
  const chars: string[] = [];
  let inDouble = false;
  let inSingle = false;
  let escaped = false;

  for (let i = 0; i < s.length; i++) {
    const ch = s[i]!;

    if (escaped) {
      chars.push(ch);
      escaped = false;
      continue;
    }

    if (ch === '\\') {
      chars.push(ch);
      escaped = true;
      continue;
    }

    if (ch === '"' && !inSingle) {
      inDouble = !inDouble;
      chars.push(ch);
      continue;
    }

    if (ch === "'" && !inDouble) {
      inSingle = !inSingle;
      chars.push('"');
      continue;
    }

    chars.push(ch);
  }

  return chars.join('');
}

/** Quote unquoted object keys: {key: "value"} → {"key": "value"} */
function quoteUnquotedKeys(s: string): string {
  // Match key positions after { or , that aren't already quoted
  return s.replace(/([{,])\s*([a-zA-Z_$][a-zA-Z0-9_$]*)\s*:/g, '$1"$2":');
}

/**
 * Fix truncated JSON by closing unmatched brackets, braces, and strings.
 * Handles cases where the LLM output was cut off mid-response.
 */
function fixTruncatedJson(s: string): string {
  let inString = false;
  let escaped = false;
  const stack: string[] = [];

  for (let i = 0; i < s.length; i++) {
    const ch = s[i]!;

    if (escaped) {
      escaped = false;
      continue;
    }

    if (ch === '\\' && inString) {
      escaped = true;
      continue;
    }

    if (ch === '"') {
      inString = !inString;
      continue;
    }

    if (!inString) {
      if (ch === '{') stack.push('}');
      else if (ch === '[') stack.push(']');
      else if (ch === '}' || ch === ']') {
        if (stack.length > 0 && stack[stack.length - 1] === ch) {
          stack.pop();
        }
      }
    }
  }

  let result = s;

  // Close unclosed string
  if (inString) {
    result += '"';
  }

  // Close all unclosed brackets/braces (in reverse order)
  while (stack.length > 0) {
    result += stack.pop();
  }

  return result;
}

/** Strip JavaScript-style comments */
function stripComments(s: string): string {
  // Remove single-line comments (not inside strings)
  let result = '';
  let inString = false;
  let escaped = false;
  let i = 0;

  while (i < s.length) {
    const ch = s[i]!;
    const next = s[i + 1];

    if (escaped) {
      result += ch;
      escaped = false;
      i++;
      continue;
    }

    if (ch === '\\' && inString) {
      result += ch;
      escaped = true;
      i++;
      continue;
    }

    if (ch === '"') {
      inString = !inString;
      result += ch;
      i++;
      continue;
    }

    if (!inString) {
      // Single-line comment
      if (ch === '/' && next === '/') {
        // Skip until newline
        while (i < s.length && s[i] !== '\n') i++;
        continue;
      }
      // Block comment
      if (ch === '/' && next === '*') {
        i += 2;
        while (i < s.length - 1 && !(s[i] === '*' && s[i + 1] === '/')) i++;
        i += 2; // skip */
        continue;
      }
    }

    result += ch;
    i++;
  }

  return result;
}

/** Escape raw newlines inside JSON string values */
function escapeNewlinesInStrings(s: string): string {
  const chars: string[] = [];
  let inString = false;
  let escaped = false;

  for (let i = 0; i < s.length; i++) {
    const ch = s[i]!;

    if (escaped) {
      chars.push(ch);
      escaped = false;
      continue;
    }

    if (ch === '\\' && inString) {
      chars.push(ch);
      escaped = true;
      continue;
    }

    if (ch === '"') {
      inString = !inString;
      chars.push(ch);
      continue;
    }

    if (inString && ch === '\n') {
      chars.push('\\n');
      continue;
    }
    if (inString && ch === '\r') {
      chars.push('\\r');
      continue;
    }

    chars.push(ch);
  }

  return chars.join('');
}

// ── Main repair function ─────────────────────────────────────────────────────

type StrategyEntry = { name: RepairStrategy; transform: (s: string) => string };

const STRATEGIES: StrategyEntry[] = [
  { name: 'markdown-code-block', transform: stripMarkdownCodeBlocks },
  { name: 'trailing-commas', transform: stripTrailingCommas },
  { name: 'single-quotes', transform: fixSingleQuotes },
  { name: 'unquoted-keys', transform: quoteUnquotedKeys },
  { name: 'truncated', transform: fixTruncatedJson },
  { name: 'comments', transform: stripComments },
  { name: 'newlines', transform: escapeNewlinesInStrings },
];

/**
 * Attempt to parse tool arguments, applying progressive repair strategies
 * if standard JSON.parse fails. Returns the parsed result and metadata about
 * whether repair was needed.
 */
export function repairToolArguments(raw: string): RepairResult {
  const trimmed = raw.trim();

  // Empty or whitespace-only → empty object
  if (!trimmed) {
    return { parsed: {}, repaired: false, strategy: null };
  }

  // Fast path: try standard parse first (most common case)
  try {
    const parsed = JSON.parse(trimmed) as Record<string, unknown>;
    return { parsed, repaired: false, strategy: null };
  } catch {
    // Fall through to repair strategies
  }

  // Apply each strategy individually, try parse after each
  for (const { name, transform } of STRATEGIES) {
    try {
      const repaired = transform(trimmed);
      const parsed = JSON.parse(repaired) as Record<string, unknown>;
      return { parsed, repaired: true, strategy: name };
    } catch {
      // This strategy alone wasn't enough, continue
    }
  }

  // Try cumulative application of all strategies
  let cumulative = trimmed;
  for (const { transform } of STRATEGIES) {
    cumulative = transform(cumulative);
  }
  try {
    const parsed = JSON.parse(cumulative) as Record<string, unknown>;
    return { parsed, repaired: true, strategy: 'truncated' }; // attribute to truncated as the likely cause
  } catch {
    // Fall through to JSON5
  }

  // Last resort: JSON5
  try {
    const parsed = JSON5.parse(trimmed) as Record<string, unknown>;
    return { parsed, repaired: true, strategy: 'json5' };
  } catch {
    // All repair strategies failed
    throw new ToolArgumentParseError(raw);
  }
}

/**
 * Sanitize a tool name by trimming whitespace and removing invalid characters.
 * Valid characters: a-z, A-Z, 0-9, underscore, hyphen.
 */
export function sanitizeToolName(name: string): string {
  return name.trim().replace(/[^a-zA-Z0-9_-]/g, '');
}

// ── Errors ───────────────────────────────────────────────────────────────────

export class ToolArgumentParseError extends Error {
  constructor(raw: string) {
    super(`Failed to parse tool arguments after all repair attempts: ${raw.substring(0, 200)}`);
    this.name = 'ToolArgumentParseError';
  }
}
