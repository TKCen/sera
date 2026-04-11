---
id: create-architecture
name: Create Architecture
version: 1.0.0
description: Design and document system architecture including component diagrams, data flows, and ADRs.
triggers: ['architecture', 'design', 'blueprint', 'adr', 'system design']
category: engineering/design
tags: ['architecture', 'design', 'adr', 'planning']
---

# Create Architecture

## Purpose

Translate business requirements and constraints into a clean, scalable technical blueprint.

## Approach

1. Clarify requirements — identify functional requirements, non-functional constraints, and expected scale.
2. Identify components — enumerate services, data stores, and external integrations needed.
3. Define data flows — document how data moves between components, including APIs and event streams.
4. Document decisions — record significant design choices as Architecture Decision Records (ADRs).
5. Review for simplicity — prefer boring, proven technology; complexity must justify itself.

## Output Format

- Component diagram (text-based or Mermaid)
- Data flow description
- ADR for each significant decision
- Open questions requiring stakeholder input

## Guidelines

- User journeys drive technical decisions — start from the user, not the implementation.
- Design for the current scale; include clear extension points for the next order of magnitude.
- Avoid distributed complexity (microservices, event sourcing) unless simpler alternatives are inadequate.
