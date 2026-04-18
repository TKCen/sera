---
name: summarise-thread
description: Produce a 3-bullet summary of a long discussion thread
inputs:
  thread_url: string
  max_bullets: string
tier: 2
---

# Behaviour

Fetch the thread, extract the core question, decisions, and unresolved
items. Cap the summary at the requested bullet count (default 3).
