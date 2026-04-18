---
name: unknown-keys
description: Exercises the warn-on-unknown-key codepath
inputs:
  foo: string
tier: 1
# The loader should log a warning for the following two keys and otherwise
# load the skill normally.
triggers:
  - invoice
  - billing
legacy_tool_binding: noop
---

Body preserved verbatim.
