import { describe, it, expect } from 'vitest';
import { stableStringify } from '../json.js';

describe('stableStringify', () => {
  it('sorts object keys alphabetically', () => {
    const a = stableStringify({ z: 1, a: 2, m: 3 });
    const b = stableStringify({ a: 2, m: 3, z: 1 });
    expect(a).toBe(b);
    expect(a).toBe('{"a":2,"m":3,"z":1}');
  });

  it('sorts nested object keys recursively', () => {
    const obj = { b: { z: 1, a: 2 }, a: { y: 3, x: 4 } };
    const result = stableStringify(obj);
    expect(result).toBe('{"a":{"x":4,"y":3},"b":{"a":2,"z":1}}');
  });

  it('preserves array order (does not sort arrays)', () => {
    const result = stableStringify({ items: [3, 1, 2] });
    expect(result).toBe('{"items":[3,1,2]}');
  });

  it('sorts keys inside array elements', () => {
    const result = stableStringify([
      { b: 1, a: 2 },
      { d: 3, c: 4 },
    ]);
    expect(result).toBe('[{"a":2,"b":1},{"c":4,"d":3}]');
  });

  it('handles null values', () => {
    expect(stableStringify(null)).toBe('null');
    expect(stableStringify({ b: null, a: 1 })).toBe('{"a":1,"b":null}');
  });

  it('handles primitive values', () => {
    expect(stableStringify('hello')).toBe('"hello"');
    expect(stableStringify(42)).toBe('42');
    expect(stableStringify(true)).toBe('true');
  });

  it('handles empty objects and arrays', () => {
    expect(stableStringify({})).toBe('{}');
    expect(stableStringify([])).toBe('[]');
  });

  it('handles deeply nested structures', () => {
    const deep = { c: { b: { a: { z: 1, y: 2 } } } };
    expect(stableStringify(deep)).toBe('{"c":{"b":{"a":{"y":2,"z":1}}}}');
  });

  it('handles circular references gracefully', () => {
    const obj: Record<string, unknown> = { a: 1 };
    obj['self'] = obj;
    const result = stableStringify(obj);
    expect(result).toBe('{"a":1,"self":"[Circular]"}');
  });

  it('supports indent parameter', () => {
    const result = stableStringify({ b: 1, a: 2 }, 2);
    expect(result).toBe('{\n  "a": 2,\n  "b": 1\n}');
  });

  it('produces identical output regardless of key insertion order', () => {
    // Simulate different insertion orders for the same logical object
    const obj1: Record<string, unknown> = {};
    obj1['role'] = 'assistant';
    obj1['content'] = 'hello';
    obj1['tool_calls'] = [{ id: '1', name: 'test' }];

    const obj2: Record<string, unknown> = {};
    obj2['tool_calls'] = [{ id: '1', name: 'test' }];
    obj2['content'] = 'hello';
    obj2['role'] = 'assistant';

    expect(stableStringify(obj1)).toBe(stableStringify(obj2));
    expect(stableStringify(obj1)).toBe(
      '{"content":"hello","role":"assistant","tool_calls":[{"id":"1","name":"test"}]}'
    );
  });
});
