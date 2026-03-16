# Development Circle — Project Context

> This document serves as the shared "constitution" for all agents in the Development Circle.
> Every agent in this circle loads this context on activation and follows its conventions.

## Technology Stack

- **Runtime**: Node.js with TypeScript (ESM modules)
- **API**: Express.js REST API
- **Database**: PostgreSQL (metadata, audit, memory) + Qdrant (vector search)
- **Real-time**: Centrifugo (WebSocket pub/sub)
- **Containerization**: Docker Compose (single-host)
- **Frontend**: Next.js with React

## Coding Conventions

- Use TypeScript strict mode for all source files
- Import paths must include `.js` extension (ESM requirement)
- Use `interface` for data shapes, `type` for unions and intersections
- Prefer `const` assertions and readonly where appropriate
- Name files in PascalCase for classes, camelCase for utilities

## Architecture Decisions

### ADR-001: Agents are Manifest-Driven
All agent behavior is defined in `AGENT.yaml` files. No hardcoded agent configurations exist in the codebase.

### ADR-002: Circle-Scoped Knowledge
Each circle maintains its own Qdrant collection and PostgreSQL schema. Knowledge does not leak between circles unless explicitly shared via bridge channels.

### ADR-003: Security Tiers
Agents operate under one of three security tiers that determine their network access, filesystem permissions, and resource limits. Tier escalation requires explicit configuration.

## API Patterns

- All API endpoints are prefixed with `/api/`
- Use standard HTTP status codes (200, 400, 404, 500)
- Error responses follow the shape: `{ error: string }`
- Success responses include relevant data directly (no wrapper envelope)

## Testing

- Unit tests use Vitest
- Test files are co-located with source files using the `.test.ts` suffix
- Integration tests may require running services (Qdrant, PostgreSQL)
