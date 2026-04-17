# SERA

The SERA project is currently being rewritten in Rust.

The original TypeScript/Node.js implementation and related legacy components have been archived in the `legacy/` directory.

For more information on the new architecture and project plan, please refer to:
- [New Implementation Plan](docs/plan/plan.md)
- [Rust Migration Plan](docs/RUST-MIGRATION-PLAN.md)

## Run locally (Rust stack)

```bash
docker compose -f docker-compose.rust.yaml up --build
```

This starts postgres (pgvector), centrifugo, ollama, and the sera-gateway on port 3001.

Check health:

```bash
curl http://localhost:3001/api/health
```

See `.omc/wiki/local-boot.md` for the full runbook including port assignments and teardown.
