import { describe, it, expect, vi, beforeEach } from 'vitest';
import { QueryBuilder } from './query-builder.js';

describe('QueryBuilder', () => {
  let qb: QueryBuilder;

  beforeEach(() => {
    qb = new QueryBuilder();
  });

  it('builds an empty WHERE clause', () => {
    expect(qb.buildWhere()).toBe('');
    expect(qb.getParams()).toEqual([]);
  });

  it('builds a WHERE clause with one condition', () => {
    qb.addCondition('id = ?', '123');
    expect(qb.buildWhere()).toBe(' WHERE id = $1');
    expect(qb.getParams()).toEqual(['123']);
  });

  it('builds a WHERE clause with multiple conditions', () => {
    qb.addCondition('status = ?', 'pending');
    qb.addCondition('agent_id = ?', 'abc');
    expect(qb.buildWhere()).toBe(' WHERE status = $1 AND agent_id = $2');
    expect(qb.getParams()).toEqual(['pending', 'abc']);
  });

  it('adds extra parameters correctly', () => {
    qb.addCondition('status = ?', 'pending');
    const limitPlaceholder = qb.addParam(50);
    const offsetPlaceholder = qb.addParam(0);

    expect(qb.buildWhere()).toBe(' WHERE status = $1');
    expect(limitPlaceholder).toBe('$2');
    expect(offsetPlaceholder).toBe('$3');
    expect(qb.getParams()).toEqual(['pending', 50, 0]);
  });

  it('handles conditions with multiple placeholders (if needed in future, but currently replaced one by one)', () => {
    // Current implementation only replaces first '?'
    // If we wanted to support multiple, we'd need a different regex.
    // Let's test current behavior.
    qb.addCondition('col1 = ? OR col2 = ?', 'val');
    // It only replaces the first one with $1. The second '?' remains.
    // Actually, string.replace(string, string) only replaces the first occurrence.
    expect(qb.buildWhere()).toBe(' WHERE col1 = $1 OR col2 = ?');
  });
});
