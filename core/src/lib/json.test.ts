import { describe, it, expect } from 'vitest';
import { parseJson, safeParseJson } from './json.js';

describe('json utility', () => {
  describe('parseJson', () => {
    it('should parse valid direct JSON', () => {
      const input = '{"key": "value"}';
      expect(parseJson(input)).toEqual({ key: 'value' });
    });

    it('should strip markdown code blocks and parse', () => {
      const input = '```json\n{"key": "value"}\n```';
      expect(parseJson(input)).toEqual({ key: 'value' });

      const input2 = '```\n{"key": "value"}\n```';
      expect(parseJson(input2)).toEqual({ key: 'value' });
    });

    it('should extract JSON object from surrounding text', () => {
      const input = 'Here is the data: {"key": "value"} and some trailing text.';
      expect(parseJson(input)).toEqual({ key: 'value' });
    });

    it('should extract JSON array from surrounding text', () => {
      const input = 'Data: [{"id": 1}, {"id": 2}] is here.';
      expect(parseJson(input)).toEqual([{ id: 1 }, { id: 2 }]);
    });

    it('should extract first valid JSON when multiple exist', () => {
      const input = 'First: {"id": 1}, Second: {"id": 2}';
      expect(parseJson(input)).toEqual({ id: 1 });
    });

    it('should throw an error for empty input', () => {
      expect(() => parseJson('')).toThrow('Empty input');
      expect(() => parseJson('   ')).toThrow('No valid JSON found in input');
    });

    it('should throw an error if no valid JSON is found', () => {
      const input = 'Just some text with no JSON.';
      expect(() => parseJson(input)).toThrow('No valid JSON found in input');
    });

    it('should handle nested structures correctly', () => {
      const input = 'Nested: {"parent": {"child": [1, 2, 3]}} text.';
      expect(parseJson(input)).toEqual({ parent: { child: [1, 2, 3] } });
    });

    it('should handle strings with braces correctly', () => {
      const input = 'Response: {"text": "This has { and } inside"}';
      expect(parseJson(input)).toEqual({ text: 'This has { and } inside' });
    });

    it('should handle escaped quotes within strings', () => {
      const input = '{"text": "Quote: \\"Hello\\""}';
      expect(parseJson(input)).toEqual({ text: 'Quote: "Hello"' });
    });
  });

  describe('safeParseJson', () => {
    it('should return parsed JSON on success', () => {
      const input = '{"key": "value"}';
      expect(safeParseJson(input, { fallback: true })).toEqual({ key: 'value' });
    });

    it('should return fallback value on failure', () => {
      const input = 'Invalid JSON data';
      const fallback = { error: true };
      expect(safeParseJson(input, fallback)).toBe(fallback);
    });

    it('should return fallback for empty input', () => {
      expect(safeParseJson('', [])).toEqual([]);
    });
  });
});
