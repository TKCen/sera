# Dogfeed Task Tracker

Tasks for building SERA's autonomous self-improvement loop.
Goal: SERA orchestrates coding agents (OMC, pi-agent+Qwen) to improve itself.

## Format

`- [ ] P<priority> | <category> | <description>` where P0=highest

## Ready (unblocked, pick from top)

### Validation Fuel (trivial tasks to test the loop on)

- [ ] P1 | lint | Replace `any` with proper type in `core/src/lib/database.ts:4`
- [ ] P2 | lint | Replace `any` types in `core/src/agents/registry.service.ts` (5 instances)
- [ ] P2 | lint | Replace `any` type in `core/src/memory/CoreMemoryService.ts:57`
- [ ] P2 | lint | Replace `any` type in `core/src/agents/manifest/AgentManifestLoader.ts:206`

### Future Infrastructure

- [ ] P2 | infra | Integrate with ScheduleService — cron-triggered cycles
- [ ] P2 | infra | Add token/cost tracking to dogfeed loop results

### Research

- [ ] P2 | research | Analyze claw-code for SERA agent runtime insights

## In Progress

## Done

<!-- Format: - [x] P<n> | <cat> | <description> | <outcome> | <tokens> | <duration> -->

- [x] P1 | lint | Remove unused `execSync` import in `core/src/agents/Orchestrator.ts:2` | FAILED: Unexpected error: [90mcore/src/agents/Orchestrator.ts[39m 141ms (unchanged)
      $ bun run typecheck && bun run lint && bun run test:web
      $ bun run --filter '_' typecheck
      core typecheck: Exited with code 0
      sera-web typecheck: Exited with code 0
      $ bun run --filter '_' lint
      sera-web lint:
      sera-web lint: Oops! Something went wrong! :(
      sera-web lint:
      sera-web lint: ESLint: 10.2.0
      sera-web lint:
      sera-web lint: Error [ERR_MODULE_NOT_FOUND]: Cannot find package 'eslint-plugin-react-hooks' imported from D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\web\eslint.config.mjs
      sera-web lint: at packageResolve (node:internal/modules/esm/resolve:873:9)
      sera-web lint: at moduleResolve (node:internal/modules/esm/resolve:946:18)
      sera-web lint: at defaultResolve (node:internal/modules/esm/resolve:1188:11)
      sera-web lint: at ModuleLoader.defaultResolve (node:internal/modules/esm/loader:642:12)
      sera-web lint: at #cachedDefaultResolve (node:internal/modules/esm/loader:591:25)
      sera-web lint: at ModuleLoader.resolve (node:internal/modules/esm/loader:574:38)
      sera-web lint: at ModuleLoader.getModuleJobForImport (node:internal/modules/esm/loader:236:38)
      sera-web lint: at ModuleJob.\_link (node:internal/modules/esm/module_job:130:49)
      sera-web lint: Exited with code 2
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\agents\Orchestrator.ts
      core lint: 180:59 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\agents\manifest\AgentManifestLoader.ts
      core lint: 206:68 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\agents\registry.service.ts
      core lint: 245:65 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 287:62 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 745:19 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 912:18 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 913:18 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\lib\database.test.ts
      core lint: 32:35 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 32:68 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\lib\database.ts
      core lint: 4:35 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 4:68 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\memory\CoreMemoryService.test.ts
      core lint: 6:17 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\memory\CoreMemoryService.ts
      core lint: 57:19 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\memory\MemoryAnalyst.test.ts
      core lint: 47:34 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 47:86 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 60:34 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 64:10 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 79:34 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 83:10 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\memory\blocks\MemoryBlockStore.test.ts
      core lint: 238:71 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 245:27 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\memory\blocks\MemoryBlockStore.ts
      core lint: 523:14 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\routes\agents.test.ts
      core lint: 12:26 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 185:77 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 206:77 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\routes\llmProxy.test.ts
      core lint: 628:69 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\routes\llmProxy.ts
      core lint: 317:48 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\routes\memory.ts
      core lint: 564:12 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\sandbox\PermissionRequestService.test.ts
      core lint: 8:21 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 9:21 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 51:19 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 68:51 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 83:19 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 94:51 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\services\vector.service.benchmark.ts
      core lint: 16:58 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 23:58 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\services\vector.service.test.ts
      core lint: 17:62 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 18:40 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 23:62 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 24:40 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 31:62 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 32:40 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 37:62 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 38:40 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 78:55 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 79:40 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 84:65 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 85:40 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 115:62 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 116:40 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 122:62 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 123:40 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 129:62 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 130:40 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 166:62 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 167:40 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 174:62 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 175:40 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 180:23 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: D:\projects\homelab\sera\.dogfeed-worktrees\dogfeed-1-remove-unused-execsync-import-in-core-sr\core\src\skills\builtins\core-memory.test.ts
      core lint: 19:10 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 29:81 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 33:20 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 45:12 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 49:20 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 64:81 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint: 68:20 warning Unexpected any. Specify a different type @typescript-eslint/no-explicit-any
      core lint:
      core lint: ✖ 66 problems (0 errors, 66 warnings)
      core lint:
      core lint: Exited with code 0
      | ~0k tokens | 35s
- [x] P0 | infra | Create `docs/DOGFEED-PROTOCOL.md` — cycle protocol for agents to follow | OK | ~5k tokens | 2min
- [x] P0 | infra | Create `docs/DOGFEED-PHASE0-LOG.md` — per-cycle tracking + retrospective | OK | ~1k tokens | 1min
- [x] P0 | infra | Build `core/src/dogfeed/analyzer.ts` — self-analyzer (reads task file + runs heuristic scans) | OK | ~10k tokens | 5min
- [x] P0 | infra | Build `core/src/dogfeed/loop.ts` — orchestrator that runs analyze→execute→verify→merge→learn | OK | ~15k tokens | 8min
- [x] P1 | infra | Build `core/src/dogfeed/agent-spawner.ts` — spawns pi-agent or OMC on a task | OK | ~10k tokens | 5min
- [x] P1 | infra | Add `POST /api/dogfeed/run` route — REST API trigger (TS reference, Rust port pending) | OK | ~5k tokens | 3min
- [x] P1 | infra | Build `core/src/dogfeed/verify-merge.ts` — runs CI, auto-merges on green | OK | ~10k tokens | 5min
