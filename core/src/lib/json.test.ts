import { describe, it, expect } from 'vitest';
import { parseJson, safeParseJson } from './json.js';

describe('json utilities', () => {
  describe('parseJson', () => {
    it('should parse clean JSON string', () => {
      const input = '{"key":"value"}';
      expect(parseJson(input)).toEqual({ key: 'value' });
    });

    it('should parse clean JSON array', () => {
      const input = '["a", "b"]';
      expect(parseJson(input)).toEqual(['a', 'b']);
    });

    it('should strip markdown code blocks and parse JSON', () => {
      const input = '```json\n{"key":"value"}\n```';
      expect(parseJson(input)).toEqual({ key: 'value' });
    });

    it('should strip generic code blocks and parse JSON', () => {
      const input = '```\n{"key":"value"}\n```';
      expect(parseJson(input)).toEqual({ key: 'value' });
    });

    it('should handle leading and trailing whitespace', () => {
      const input = '   \n\n  {"key":"value"} \n\n  ';
      expect(parseJson(input)).toEqual({ key: 'value' });
    });

    it('should extract JSON embedded within text', () => {
      const input = 'Here is the response: {"key":"value"} and some trailing text.';
      expect(parseJson(input)).toEqual({ key: 'value' });
    });

    it('should extract JSON array embedded within text', () => {
      const input = 'Here is the response: ["a", "b"] and some trailing text.';
      expect(parseJson(input)).toEqual(['a', 'b']);
    });

    it('should throw Error on empty input', () => {
      expect(() => parseJson('')).toThrow('Empty input');
      expect(() => parseJson(null as unknown as string)).toThrow('Empty input');
    });

    it('should throw Error if no valid JSON found', () => {
      const input = 'Just some text, no JSON here.';
      expect(() => parseJson(input)).toThrow(/No valid JSON found in input/);
    });
  });

  describe('safeParseJson', () => {
    it('should return parsed JSON when valid', () => {
      const input = '{"key":"value"}';
      expect(safeParseJson(input, { fallback: true })).toEqual({ key: 'value' });
    });

    it('should return fallback when parsing fails', () => {
      const input = 'invalid json';
      expect(safeParseJson(input, { fallback: true })).toEqual({ fallback: true });
    });

    it('should return fallback on empty input', () => {
      expect(safeParseJson('', { fallback: true })).toEqual({ fallback: true });
    });
  });
});
