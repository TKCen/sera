---
id: adversarial-review
name: Adversarial Review
version: 1.0.0
description: Stress-test a design or implementation by actively seeking failure modes, security gaps, and unexamined assumptions.
triggers: ['adversarial', 'red team', 'attack', 'failure modes', 'stress test', 'critique']
category: engineering/review
tags: ['review', 'security', 'reliability', 'adversarial']
---

# Adversarial Review

## Purpose

Adopt a critical, adversarial mindset to find weaknesses in a design or implementation before they reach production. This is distinct from a standard code review — the goal is to break things, not approve them.

## Attack Vectors to Explore

### Correctness

- What inputs or sequences of events will cause incorrect behaviour?
- What race conditions or ordering assumptions exist?
- What happens at boundary values and empty states?

### Security

- What happens if an attacker controls any input field?
- Are there injection vectors (SQL, shell, prompt)?
- Are authentication and authorisation checks applied consistently?
- Is sensitive data logged, cached, or leaked?

### Reliability

- What single points of failure exist?
- What happens when a downstream dependency is slow or unavailable?
- Is the system recoverable after a crash mid-operation?

### Scalability

- What breaks first under 10x or 100x load?
- Are there O(n²) or worse operations on potentially large inputs?

## Output Format

- Numbered list of findings, each with: description, impact (critical/high/medium/low), and recommended mitigation.
- Summary: overall risk assessment and top 3 concerns.
