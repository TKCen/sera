# Database Migration Policy

All migrations in `core/src/db/migrations/` **must be idempotent**. Running a migration
twice against a database that already applied it must be a no-op with no errors.

This matters for SERA because:
- The `pgmigrations` tracking table can be partially reset while the data
  volume is retained (e.g. `docker compose down` without `-v` followed by
  re-creating the container).
- CI pipelines may replay migrations against a pre-populated database.
- Manual debugging sometimes requires re-running specific migrations.

---

## Rules

### 1. Tables — always use `ifNotExists`

```js
// ✅ Correct
pgm.createTable('my_table', { ... }, { ifNotExists: true });

// ❌ Wrong — crashes if table exists
pgm.createTable('my_table', { ... });
```

### 2. Indexes — always use `CREATE INDEX IF NOT EXISTS`

`pgm.createIndex()` does **not** support `ifNotExists` reliably across all
node-pg-migrate versions. Use raw SQL instead:

```js
// ✅ Correct
pgm.sql('CREATE INDEX IF NOT EXISTS my_idx ON my_table (col)');
pgm.sql('CREATE UNIQUE INDEX IF NOT EXISTS my_unique_idx ON my_table (col1, col2)');

// ❌ Wrong — crashes on duplicate
pgm.createIndex('my_table', ['col']);
```

### 3. Adding columns — always use `ADD COLUMN IF NOT EXISTS`

```js
// ✅ Correct
pgm.sql('ALTER TABLE my_table ADD COLUMN IF NOT EXISTS new_col text');

// ❌ Wrong — crashes if column exists
pgm.addColumn('my_table', { new_col: { type: 'text' } });
```

For multiple columns, batch them:
```js
pgm.sql(`
  ALTER TABLE my_table
    ADD COLUMN IF NOT EXISTS col_a text,
    ADD COLUMN IF NOT EXISTS col_b int DEFAULT 0;
`);
```

### 4. Extensions — always use `IF NOT EXISTS`

```js
// ✅ Correct
pgm.sql('CREATE EXTENSION IF NOT EXISTS vector');

// ❌ Wrong
pgm.sql('CREATE EXTENSION vector');
```

### 5. `down` migrations are exempt

The `exports.down` function is only called deliberately during a rollback. It
does **not** need to be idempotent. Use `ifExists: true` as a courtesy:

```js
exports.down = (pgm) => {
  pgm.dropTable('my_table');               // fine — rollback is intentional
  pgm.dropIndex('my_table', [], { name: 'my_idx', ifExists: true }); // courtesy
};
```

---

## New Migration Checklist

Before committing a new migration file, verify:

- [ ] Every `pgm.createTable(...)` call has `{ ifNotExists: true }` as the third argument
- [ ] Every index creation uses `pgm.sql('CREATE INDEX IF NOT EXISTS ...')`
- [ ] Every `pgm.addColumn(...)` is replaced with `pgm.sql('ALTER TABLE ... ADD COLUMN IF NOT EXISTS ...')`
- [ ] Every `pgm.sql('CREATE EXTENSION ...')` uses `IF NOT EXISTS`
- [ ] The migration file comment references `docs/MIGRATIONS.md`

---

## Migration File Template

```js
/**
 * Migration: Epic NN — Short Description
 * Story N.N: What this adds.
 * See docs/MIGRATIONS.md — all DDL is idempotent.
 */

exports.up = (pgm) => {
  pgm.createTable('my_table', {
    id: { type: 'uuid', primaryKey: true, default: pgm.func('gen_random_uuid()') },
    // ... columns
  }, { ifNotExists: true });

  pgm.sql(`
    CREATE INDEX IF NOT EXISTS my_table_col_idx ON my_table (col);
  `);

  // For column additions to existing tables:
  pgm.sql('ALTER TABLE other_table ADD COLUMN IF NOT EXISTS new_col text');
};

exports.down = (pgm) => {
  pgm.dropTable('my_table');
};
```
