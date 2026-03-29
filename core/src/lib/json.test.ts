import { describe, it, expect } from 'vitest';
import { parseJson, safeParseJson } from './json.js';

describe('JSON Parsing Utilities', () => {
  describe('parseJson', () => {
    it('throws error for empty input', () => {
      expect(() => parseJson('')).toThrow('Empty input');
      expect(() => parseJson(null as any)).toThrow('Empty input');
      expect(() => parseJson(undefined as any)).toThrow('Empty input');
    });

    it('parses direct JSON successfully', () => {
      expect(parseJson('{"hello": "world"}')).toEqual({ hello: 'world' });
      expect(parseJson('[1, 2, 3]')).toEqual([1, 2, 3]);
      expect(parseJson('true')).toEqual(true);
      expect(parseJson('42')).toEqual(42);
    });

    it('strips markdown code blocks', () => {
      const input1 = '```json\n{"hello": "world"}\n```';
      expect(parseJson(input1)).toEqual({ hello: 'world' });

      const input2 = '```\n[1, 2, 3]\n```';
      expect(parseJson(input2)).toEqual([1, 2, 3]);

      const input3 = '```json {"hello": "world"} ```';
      expect(parseJson(input3)).toEqual({ hello: 'world' });
    });

    it('extracts JSON when there is leading or trailing text', () => {
      const input1 = 'Here is the JSON you requested: {"hello": "world"} Hope that helps!';
      expect(parseJson(input1)).toEqual({ hello: 'world' });

      const input2 = 'I found the array: [1, 2, 3] and nothing else.';
      expect(parseJson(input2)).toEqual([1, 2, 3]);

      const input3 = 'Start\n{\n  "nested": { "a": 1 }\n}\nEnd';
      expect(parseJson(input3)).toEqual({ nested: { a: 1 } });
    });

    it('handles escaped strings correctly during extraction', () => {
      const input = 'Prefix { "key": "value \\"with quotes\\"" } Suffix';
      expect(parseJson(input)).toEqual({ key: 'value "with quotes"' });
    });

    it('handles nested objects and arrays correctly', () => {
      const input = 'Some text { "arr": [1, { "a": 2 }, 3] } More text';
      expect(parseJson(input)).toEqual({ arr: [1, { a: 2 }, 3] });
    });

    it('throws error if extracted JSON is invalid', () => {
      const input = 'Prefix { "bad": "json } Suffix';
      // If the closing brace is missing or invalid, it might throw "Failed to parse..."
      // or fall back to throwing "No valid JSON found..." depending on the exact string.
      // We just need to assert that it throws an error in general for invalid JSON.
      expect(() => parseJson(input)).toThrow();
    });

    it('throws error if no valid JSON found', () => {
      const input = 'Just some regular text without any json.';
      expect(() => parseJson(input)).toThrow(/No valid JSON found in input/);
    });
  });

  describe('safeParseJson', () => {
    it('returns parsed JSON on success', () => {
      expect(safeParseJson('{"hello": "world"}', { fallback: true })).toEqual({ hello: 'world' });
    });

    it('returns fallback value on failure', () => {
      expect(safeParseJson('Not JSON', { fallback: true })).toEqual({ fallback: true });
      expect(safeParseJson('', { fallback: true })).toEqual({ fallback: true });
      expect(safeParseJson('Prefix { "bad": "json } Suffix', [])).toEqual([]);
    });
  });
});
