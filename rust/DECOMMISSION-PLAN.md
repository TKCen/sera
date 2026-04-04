# Phase 7: TypeScript Decommission Plan

## Prerequisites (all must be true)

- [ ] All API endpoints migrated to Rust and serving production traffic
- [ ] Reverse proxy (Phase 5) routing 100% of traffic to Rust backend
- [ ] Dual-run comparison clean for 2+ weeks (no response divergence)
- [ ] Rollback practiced at least once (proxy switch back to TS)
- [ ] sera-runtime (Rust) producing equivalent output to TS agent-runtime
- [ ] sera-tui (Rust) at feature parity with Go TUI
- [ ] All integration tests passing against Rust backend only

## Step 1: Remove TS from Docker Compose

```yaml
# In docker-compose.yaml, remove or comment out:
# - sera-core service definition
# - sera-core-dev service definition (if in dev overlay)
# - node_modules_core named volume

# In docker-compose.dev.yaml, remove:
# - core/docker-entrypoint.dev.sh mount
# - node_modules_core volume mount
```

## Step 2: Archive TypeScript Source

```bash
# Create archive branch
git checkout -b archive/typescript-core
git checkout main

# Remove directories
rm -rf core/
rm -rf core/agent-runtime/

# Keep reference in docs
echo "TypeScript core archived to branch: archive/typescript-core" >> docs/MIGRATION-LOG.md
```

## Step 3: Update CI Pipeline

```yaml
# .github/workflows/ci.yml changes:
# - Remove Node.js setup step
# - Remove npm/bun install for core/
# - Remove core typecheck/lint/test jobs
# - Add Rust build + test + clippy jobs
# - Update the 'validate' job name (keep same for branch protection)
```

Proposed CI job:

```yaml
validate:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - uses: Swatinem/rust-cache@v2
      with:
        workspaces: rust
    - name: Check
      run: cd rust && cargo check --workspace
    - name: Clippy
      run: cd rust && cargo clippy --workspace -- -D warnings
    - name: Test
      run: cd rust && cargo test --workspace
    - name: Web lint + typecheck
      run: cd web && bun install && bun run lint && bun run typecheck
```

## Step 4: Update Web API Client

Review `web/src/lib/api/` for any TS-specific endpoint assumptions:

- Snake_case vs camelCase response field mapping
- Any endpoints that existed only in TS
- WebSocket/SSE connection URLs

## Step 5: Update Documentation

- `docs/ARCHITECTURE.md` — update tech stack section
- `CLAUDE.md` — remove TS-specific learnings, add Rust sections
- `core/CLAUDE.md` → `rust/CLAUDE.md` (already exists)
- Remove `core/agent-runtime/CLAUDE.md`
- Update `docker-compose.yaml` comments

## Step 6: Clean Up

- Remove `package.json` and `package-lock.json` from root (if only for core)
- Remove `tsconfig.json` for core
- Remove `core/tsup.config.ts`
- Remove TS-specific GitHub Actions caches
- Remove Go TUI (`tui/` directory) after Rust TUI is validated

## Rollback Plan

If issues found after decommission:

1. `git revert` the removal commits
2. Re-add sera-core to docker-compose
3. Switch proxy back to TS backend
4. Investigate and fix the Rust issue
5. Re-attempt decommission

## Timeline

| Step                       | Duration     | Dependency     |
| -------------------------- | ------------ | -------------- |
| Prerequisites verification | 2 weeks      | Dual-run clean |
| Step 1: Docker compose     | 1 day        | Prerequisites  |
| Step 2: Archive source     | 1 day        | Step 1         |
| Step 3: CI update          | 1 day        | Step 2         |
| Step 4: Web client         | 2-3 days     | Step 1         |
| Step 5: Documentation      | 1 day        | Step 3         |
| Step 6: Cleanup            | 1 day        | Step 5         |
| **Total**                  | **~3 weeks** |                |
