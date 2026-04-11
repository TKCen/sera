import { describe, it, expect } from 'vitest';
import { repairToolArguments, sanitizeToolName, ToolArgumentParseError } from '../toolArgumentRepair.js';

describe('repairToolArguments', () => {
  describe('fast path — valid JSON', () => {
    it('parses valid JSON without repair', () => {
      const result = repairToolArguments('{"key": "value", "num": 42}');
      expect(result.parsed).toEqual({ key: 'value', num: 42 });
      expect(result.repaired).toBe(false);
      expect(result.strategy).toBeNull();
    });

    it('handles empty string as empty object', () => {
      const result = repairToolArguments('');
      expect(result.parsed).toEqual({});
      expect(result.repaired).toBe(false);
    });

    it('handles whitespace-only as empty object', () => {
      const result = repairToolArguments('   \n  ');
      expect(result.parsed).toEqual({});
      expect(result.repaired).toBe(false);
    });

    it('parses valid nested JSON', () => {
      const result = repairToolArguments('{"a": {"b": [1, 2, 3]}, "c": true}');
      expect(result.parsed).toEqual({ a: { b: [1, 2, 3] }, c: true });
      expect(result.repaired).toBe(false);
    });
  });

  describe('trailing commas', () => {
    it('fixes trailing comma in object', () => {
      const result = repairToolArguments('{"key": "value",}');
      expect(result.parsed).toEqual({ key: 'value' });
      expect(result.repaired).toBe(true);
      expect(result.strategy).toBe('trailing-commas');
    });

    it('fixes trailing comma in array', () => {
      const result = repairToolArguments('{"arr": [1, 2, 3,]}');
      expect(result.parsed).toEqual({ arr: [1, 2, 3] });
      expect(result.repaired).toBe(true);
    });

    it('fixes multiple trailing commas', () => {
      const result = repairToolArguments('{"a": 1, "b": [1, 2,], "c": 3,}');
      expect(result.parsed).toEqual({ a: 1, b: [1, 2], c: 3 });
      expect(result.repaired).toBe(true);
    });
  });

  describe('single quotes', () => {
    it('fixes single-quoted keys and values', () => {
      const result = repairToolArguments("{'key': 'value'}");
      expect(result.parsed).toEqual({ key: 'value' });
      expect(result.repaired).toBe(true);
      expect(result.strategy).toBe('single-quotes');
    });

    it('handles mixed quotes', () => {
      const result = repairToolArguments("{'key': \"value\"}");
      expect(result.parsed).toEqual({ key: 'value' });
      expect(result.repaired).toBe(true);
    });
  });

  describe('unquoted keys', () => {
    it('quotes unquoted keys', () => {
      const result = repairToolArguments('{key: "value", count: 42}');
      expect(result.parsed).toEqual({ key: 'value', count: 42 });
      expect(result.repaired).toBe(true);
      expect(result.strategy).toBe('unquoted-keys');
    });

    it('handles underscore and dollar in key names', () => {
      const result = repairToolArguments('{_private: true, $special: "yes"}');
      expect(result.parsed).toEqual({ _private: true, $special: 'yes' });
      expect(result.repaired).toBe(true);
    });
  });

  describe('truncated JSON', () => {
    it('closes truncated object', () => {
      const result = repairToolArguments('{"key": "value"');
      expect(result.parsed).toEqual({ key: 'value' });
      expect(result.repaired).toBe(true);
    });

    it('closes truncated string and object', () => {
      const result = repairToolArguments('{"key": "val');
      expect(result.parsed).toEqual({ key: 'val' });
      expect(result.repaired).toBe(true);
    });

    it('closes truncated nested structure', () => {
      const result = repairToolArguments('{"a": {"b": [1, 2');
      expect(result.parsed).toEqual({ a: { b: [1, 2] } });
      expect(result.repaired).toBe(true);
    });
  });

  describe('comments', () => {
    it('strips single-line comments', () => {
      const result = repairToolArguments('{"key": "value" // this is a comment\n}');
      expect(result.parsed).toEqual({ key: 'value' });
      expect(result.repaired).toBe(true);
    });

    it('strips block comments', () => {
      const result = repairToolArguments('{"key": "value" /* comment */}');
      expect(result.parsed).toEqual({ key: 'value' });
      expect(result.repaired).toBe(true);
    });
  });

  describe('newlines in strings', () => {
    it('escapes raw newlines inside string values', () => {
      const result = repairToolArguments('{"text": "line1\nline2"}');
      expect(result.parsed).toEqual({ text: 'line1\nline2' });
      expect(result.repaired).toBe(true);
    });
  });

  describe('JSON5 fallback', () => {
    it('parses JSON5 with hex numbers', () => {
      const result = repairToolArguments('{key: 0xFF}');
      expect(result.parsed).toEqual({ key: 255 });
      expect(result.repaired).toBe(true);
      expect(result.strategy).toBe('json5');
    });

    it('parses JSON5 with Infinity', () => {
      const result = repairToolArguments('{key: Infinity}');
      expect(result.parsed).toEqual({ key: Infinity });
      expect(result.repaired).toBe(true);
      expect(result.strategy).toBe('json5');
    });
  });

  describe('error handling', () => {
    it('throws ToolArgumentParseError for completely unparseable input', () => {
      expect(() => repairToolArguments('not json at all <<<>>>')).toThrow(ToolArgumentParseError);
    });
  });
});

describe('sanitizeToolName', () => {
  it('trims whitespace', () => {
    expect(sanitizeToolName('  shell-exec  ')).toBe('shell-exec');
  });

  it('removes invalid characters', () => {
    expect(sanitizeToolName('shell exec!!')).toBe('shellexec');
  });

  it('preserves hyphens and underscores', () => {
    expect(sanitizeToolName('file-read_v2')).toBe('file-read_v2');
  });

  it('handles empty string', () => {
    expect(sanitizeToolName('')).toBe('');
  });

  it('removes spaces between words', () => {
    expect(sanitizeToolName('my tool name')).toBe('mytoolname');
  });
});
