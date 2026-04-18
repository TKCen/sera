# Circles & Coordination

Circles are named groups of agents that collaborate on shared goals. They provide team structure, shared knowledge, and broadcast communication channels.

## What a Circle Provides

| Feature                   | Description                                                     |
| ------------------------- | --------------------------------------------------------------- |
| **Shared knowledge base** | Git-backed knowledge repository with per-agent attribution      |
| **Broadcast channels**    | Named channels for team communication (e.g., `dev-alerts`)      |
| **Pooled budgets**        | Optional shared token budgets across circle members             |
| **Constitution**          | Shared rules and principles injected into member system prompts |
| **Party mode**            | Coordinated multi-agent discussion with orchestrator selection  |

## Circle Definition

Circles are defined in YAML files in the `circles/` directory:

```yaml title="circles/development.circle.yaml"
apiVersion: sera/v1
kind: Circle
metadata:
  name: development
  displayName: Development Team

members:
  - architect-prime
  - developer-prime
  - researcher-prime

channels:
  - name: dev-general
    displayName: General Development
    persistence: persistent
  - name: dev-alerts
    displayName: Alerts
    persistence: persistent
  - name: dev-reviews
    displayName: Code Reviews
    persistence: persistent

knowledge:
  qdrantCollection: development-knowledge
  schema: circle_development

partyMode:
  enabled: true
  orchestrator: architect-prime
  selectionStrategy: relevance
```

## Knowledge Scoping

Circle knowledge uses git for version control and conflict resolution:

- Each agent commits to its own branch: `knowledge/agent-{instanceId}`
- Commits carry the agent's identity as git committer
- Merging to `main` requires either `merge-without-approval` capability or operator approval
- Qdrant indexes are maintained per-namespace (`circle:{circleId}`)

The **system circle** is a built-in circle that all agents can read from. It serves as the global knowledge base. Sera has write access by default.

## Party Mode

When party mode is enabled, circles support coordinated multi-agent discussions:

1. A topic is posted to a circle channel
2. The orchestrator agent evaluates which members are relevant
3. Selected agents receive the topic and contribute their perspective
4. The orchestrator synthesises responses and produces a summary

This enables structured brainstorming, design reviews, and collaborative problem-solving across agent specialisations.

## Inter-Circle Communication

Circles can bridge channels between them:

```yaml
connections:
  - targetCircle: development
    bridgedChannels:
      - deployment-requests
      - incident-reports
```

This allows the operations circle to send deployment requests to the development circle without agents being members of both.
