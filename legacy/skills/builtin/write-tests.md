---
id: write-tests
name: Write Tests
version: 1.0.0
description: Write comprehensive, maintainable tests for a given unit, integration point, or user journey.
triggers: ['test', 'tests', 'unit test', 'integration test', 'coverage', 'tdd']
category: engineering/development
tags: ['testing', 'tdd', 'quality']
---

# Write Tests

## Purpose

Produce a test suite that gives genuine confidence in the correctness of code and catches regressions early.

## Test Levels

### Unit tests

- Test a single function or class in isolation.
- Mock external dependencies.
- Cover: happy path, error paths, boundary values, empty/null inputs.

### Integration tests

- Test the interaction between two or more real components.
- Use real infrastructure (database, queue) where practical via test containers or in-memory equivalents.
- Cover: contract correctness between components.

### End-to-end tests

- Test a full user journey through the system.
- Use sparingly — they are slow and brittle.
- Cover: the most critical user-facing flows only.

## Guidelines

- Tests are first-class code — apply the same quality standards as production code.
- Each test should have one clear reason to fail.
- Test names should describe the scenario: `should return 404 when agent does not exist`.
- Avoid testing implementation details; test observable behaviour.
- Do not use sleep/wait hacks — use proper async/await or event-driven coordination.

## Structure (Arrange-Act-Assert)

```
// Arrange — set up the inputs and context
// Act — call the thing under test
// Assert — verify the outcome
```
