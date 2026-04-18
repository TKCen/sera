---
id: check-implementation-readiness
name: Check Implementation Readiness
version: 1.0.0
description: Verify that a design or specification is sufficiently complete and unambiguous to hand off to an implementation team.
triggers: ['ready', 'readiness', 'handoff', 'spec review', 'implementation ready']
category: engineering/design
tags: ['architecture', 'review', 'readiness', 'handoff']
---

# Check Implementation Readiness

## Purpose

Assess whether a design document, epic, or specification is ready for an implementation team to begin work without requiring repeated clarification.

## Checklist

### Functional completeness

- [ ] All user-facing behaviours are described with clear acceptance criteria.
- [ ] Edge cases and error states are documented.
- [ ] Scope boundaries are explicit (what is in/out of scope).

### Technical clarity

- [ ] Data models and API contracts are fully defined.
- [ ] External dependencies and integration points are identified.
- [ ] Security and access control requirements are stated.

### Operability

- [ ] Observability requirements (logs, metrics, traces) are specified.
- [ ] Deployment and migration steps are outlined.
- [ ] Rollback strategy exists for risky changes.

### Risks

- [ ] Known unknowns are listed.
- [ ] High-risk areas are flagged for extra review.

## Output

A readiness report listing: what is ready, what is incomplete, and what must be resolved before implementation begins.
