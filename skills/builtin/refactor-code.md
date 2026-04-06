---
id: refactor-code
name: Refactor Code
version: 1.0.0
description: Improve code structure, readability, and maintainability without changing observable behaviour.
triggers: ['refactor', 'clean up', 'restructure', 'simplify', 'extract']
category: engineering/development
tags: ['refactoring', 'code quality', 'maintenance']
---

# Refactor Code

## Purpose

Improve the internal quality of code without changing what it does. A refactor that changes behaviour is a bug, not a refactor.

## When to Refactor

- Duplication: the same logic appears in multiple places.
- Long functions: a function does more than one thing.
- Unclear naming: names don't reveal intent.
- Deep nesting: logic is hard to follow due to indentation depth.
- Tight coupling: a change in one place requires changes in many others.

## Process

### 1. Establish a safety net

- Confirm that tests exist covering the code to be refactored.
- If tests are missing, write characterisation tests before refactoring.

### 2. Make atomic moves

- One refactoring operation per commit: extract function, rename, move file, etc.
- Each step must leave the tests green before proceeding to the next.

### 3. Verify behaviour is preserved

- Run the full test suite after each atomic move.
- Do not rely on "it looks right" — tests must confirm it.

## Common Operations

- Extract function: pull a block of logic into a named function.
- Rename: give variables and functions names that reveal intent.
- Inline: remove a named intermediate that adds no clarity.
- Move: relocate a function or module to where it logically belongs.
- Flatten: replace nested conditionals with early returns.
