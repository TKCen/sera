---
id: implement-feature
name: Implement Feature
version: 1.0.0
description: Implement a new feature end-to-end — from reading the spec to writing production-ready code with tests.
triggers: ['implement', 'feature', 'build', 'develop', 'code']
category: engineering/development
tags: ['implementation', 'feature', 'development']
---

# Implement Feature

## Purpose

Deliver a working, tested, production-ready implementation of a specified feature.

## Process

### 1. Understand the requirement

- Read the spec, issue, or acceptance criteria in full before writing any code.
- Identify ambiguities and resolve them before proceeding (ask if needed).
- Locate the relevant parts of the codebase using file-read and knowledge-query.

### 2. Plan the change

- Identify all files that need to change.
- Determine the smallest viable diff that satisfies acceptance criteria.
- Flag any migrations, API contract changes, or breaking changes upfront.

### 3. Implement

- Write tests first (or alongside) for new behaviour.
- Match existing code style, naming conventions, and error handling patterns.
- Keep functions small and focused; prefer composition.

### 4. Verify

- Ensure all tests pass.
- Run the linter/formatter.
- Manually verify the happy path and at least one error path.

## Guidelines

- Make it work, make it right, make it fast — in that order.
- Do not broaden scope beyond the requested feature.
- Leave no debug code, TODOs, or placeholder comments in the final diff.
