/**
 * Robust JSON parsing utility.
 * Handles common LLM formatting issues like markdown blocks,
 * leading/trailing text, and extra whitespace.
 */

/**
 * Extracts and parses the first JSON object or array found in a string.
 */
export function parseJson<T = any>(text: string): T {
  if (!text) {
    throw new Error('Empty input');
  }

  let cleaned = text.trim();

  // 1. Strip markdown code blocks if present
  // Matches ```json ... ``` or ``` ... ```
  const codeBlockMatch = cleaned.match(/^```(?:json)?\s*([\s\S]*?)\s*```$/m)
                      || cleaned.match(/```(?:json)?\s*([\s\S]*?)\s*```/);

  if (codeBlockMatch && codeBlockMatch[1]) {
    cleaned = codeBlockMatch[1].trim();
  }

  // 2. Try direct parse first
  try {
    return JSON.parse(cleaned);
  } catch (err) {
    // 3. If direct parse fails, try to extract the first valid JSON structure
    // We'll use a simple stack-based approach to find the matching closing brace/bracket
    const firstBrace = cleaned.indexOf('{');
    const firstBracket = cleaned.indexOf('[');

    let start = -1;
    let openChar = '';
    let closeChar = '';

    if (firstBrace !== -1 && (firstBracket === -1 || firstBrace < firstBracket)) {
      start = firstBrace;
      openChar = '{';
      closeChar = '}';
    } else if (firstBracket !== -1) {
      start = firstBracket;
      openChar = '[';
      closeChar = ']';
    }

    if (start !== -1) {
      let stack = 0;
      let inString = false;
      let escaped = false;

      for (let i = start; i < cleaned.length; i++) {
        const char = cleaned[i];

        if (escaped) {
          escaped = false;
          continue;
        }

        if (char === '\\') {
          escaped = true;
          continue;
        }

        if (char === '"') {
          inString = !inString;
          continue;
        }

        if (!inString) {
          if (char === openChar) {
            stack++;
          } else if (char === closeChar) {
            stack--;
            if (stack === 0) {
              const potentialJson = cleaned.substring(start, i + 1);
              try {
                return JSON.parse(potentialJson);
              } catch (innerErr) {
                // If it fails, maybe it wasn't the right matching pair (unlikely with this logic)
                // but we keep looking just in case or throw.
                throw new Error(`Failed to parse extracted JSON: ${innerErr instanceof Error ? innerErr.message : String(innerErr)}`);
              }
            }
          }
        }
      }
    }

    throw new Error(`No valid JSON found in input: ${err instanceof Error ? err.message : String(err)}`);
  }
}

/**
 * Safely attempts to parse JSON, returning a fallback value on failure.
 */
export function safeParseJson<T = any>(text: string, fallback: T): T {
  try {
    return parseJson<T>(text);
  } catch {
    return fallback;
  }
}
