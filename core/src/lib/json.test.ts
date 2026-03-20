import { describe, it, expect } from 'vitest';
import { parseJson } from './json.js';

describe('parseJson', () => {
  it('should parse valid JSON', () => {
    const input = '{"foo": "bar"}';
    expect(parseJson(input)).toEqual({ foo: 'bar' });
  });

  it('should parse JSON wrapped in markdown blocks', () => {
    const input = '```json\n{"foo": "bar"}\n```';
    expect(parseJson(input)).toEqual({ foo: 'bar' });
  });

  it('should parse JSON wrapped in markdown blocks without "json" label', () => {
    const input = '```\n{"foo": "bar"}\n```';
    expect(parseJson(input)).toEqual({ foo: 'bar' });
  });

  it('should extract and parse JSON from surrounding text', () => {
    const input = 'Here is the result: {"foo": "bar"} Hope this helps!';
    expect(parseJson(input)).toEqual({ foo: 'bar' });
  });

  it('should extract and parse JSON from surrounding text with markdown', () => {
    const input =
      'Sure, here is the JSON:\n\n```json\n{"foo": "bar"}\n```\n\nLet me know if you need anything else.';
    expect(parseJson(input)).toEqual({ foo: 'bar' });
  });

  it('should handle arrays', () => {
    const input = '[1, 2, 3]';
    expect(parseJson(input)).toEqual([1, 2, 3]);
  });

  it('should extract arrays from surrounding text', () => {
    const input = 'Result: [1, 2, 3]';
    expect(parseJson(input)).toEqual([1, 2, 3]);
  });

  it('should handle trailing text with braces correctly', () => {
    const input = 'The JSON is: {"foo": "bar"} and some more text with } braces';
    expect(parseJson(input)).toEqual({ foo: 'bar' });
  });

  it('should handle nested structures correctly', () => {
    const input = 'Nested: {"foo": {"bar": "baz"}} tail';
    expect(parseJson(input)).toEqual({ foo: { bar: 'baz' } });
  });

  it('should handle strings with escaped quotes', () => {
    const input = 'JSON: {"foo": "bar \\"quote\\" baz"}';
    expect(parseJson(input)).toEqual({ foo: 'bar "quote" baz' });
  });

  it('should throw error for empty input', () => {
    expect(() => parseJson('')).toThrow('Empty input');
  });

  it('should throw error for invalid JSON', () => {
    expect(() => parseJson('not json')).toThrow('No valid JSON found in input');
  });

  it('should throw error for malformed extracted JSON', () => {
    const input = 'Result: {"foo": "bar", }'; // Trailing comma is invalid in standard JSON
    expect(() => parseJson(input)).toThrow('Failed to parse extracted JSON');
  });
});
