/**
 * A simple utility to build dynamic SQL WHERE clauses safely.
 * Handles parameter indexing ($1, $2, etc.) automatically.
 */
export class QueryBuilder {
  private conditions: string[] = [];
  private params: unknown[] = [];

  /**
   * Adds a condition with a single parameter.
   * Use '?' as a placeholder in the condition string.
   * Example: qb.addCondition('status = ?', 'pending')
   */
  addCondition(condition: string, value: unknown): this {
    this.params.push(value);
    const placeholder = `$${this.params.length}`;
    this.conditions.push(condition.replace('?', placeholder));
    return this;
  }

  /**
   * Builds the WHERE clause, including the 'WHERE' keyword.
   * Returns an empty string if no conditions were added.
   * Returns with a leading space if not empty.
   */
  buildWhere(): string {
    return this.conditions.length > 0 ? ` WHERE ${this.conditions.join(' AND ')}` : '';
  }

  /**
   * Returns the accumulated parameters.
   */
  getParams(): unknown[] {
    return [...this.params];
  }

  /**
   * Adds a parameter and returns its placeholder (e.g., '$3').
   * Useful for LIMIT, OFFSET, or other parts of the query that aren't in the WHERE clause.
   */
  addParam(value: unknown): string {
    this.params.push(value);
    return `$${this.params.length}`;
  }
}
