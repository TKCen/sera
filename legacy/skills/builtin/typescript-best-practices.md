---
id: typescript-best-practices
name: TypeScript Best Practices
version: 1.0.0
description: Guidance on writing clean, safe, and idiomatic TypeScript.
triggers: ["typescript", "ts", "coding"]
category: engineering/typescript
tags: ["typescript", "best-practices", "guidance"]
---

# TypeScript Best Practices

## Type Safety
- Avoid `any`. Use `unknown` and narrow with type guards.
- Prefer `interface` for public API shapes, `type` for unions and mapped types.
- Enable `strict: true` in tsconfig — never disable it per-file.

## Async Patterns
- Always `await` or explicitly discard Promises (`void asyncFn()`).
- Use `Promise.all` for concurrent independent operations.
- Never mix callbacks and Promises in the same control flow.

## Error Handling
- Use typed error classes extending `Error`.
- Wrap external I/O in explicit try/catch — never let rejections bubble silently.
