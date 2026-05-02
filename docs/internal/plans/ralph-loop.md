# SERA Ralph Loop Prompt

> **Purpose:** Project-specific guidance for any ralph iteration on the SERA codebase.  
> **Usage:** Copy the prompt below into a ralph session, then append your specific task.  

---

## Starting Prompt

```
ralph do:

0a. Study @docs/plan/specs/* to learn about the SERA specifications. The spec index is 
at @docs/plan/specs/README.md. These are the canonical architecture specs for the system.

0b. Study @docs/plan/plan.md — this is the PRD (Product Requirements Document) that the 
specs are derived from. It contains the full vision, design philosophy, and architectural 
reference.

0c. Study @docs/plan/mvs-review-plan.md — this contains the MVS (Minimal Viable SERA) 
definition, the 8-crate subset, acceptance test, deferral list, and TS-to-Rust tool 
mapping. This tells you what to build first vs what to defer.

0d. The current TypeScript codebase lives in @core/ (API server) and 
@core/agent-runtime/ (agent worker). This is the reference implementation. Key files:
- @core/agent-runtime/src/tools/definitions.ts — tool surface
- @core/agent-runtime/src/loop.ts — reasoning/turn loop  
- @core/agent-runtime/src/contextManager.ts — context assembly
- @core/src/channels/adapters/DiscordAdapter.ts — Discord integration
- @core/agent-runtime/src/llmClient.ts — model provider integration

0e. The Rust workspace is being built. Check @tui/src/ and any crates/ directory for 
existing Rust code. Before implementing anything, search the codebase to confirm it 
doesn't already exist.

1. [INSERT YOUR SPECIFIC TASK HERE]

2. After implementing functionality or resolving problems, run the tests for that unit 
of code that was improved. If functionality is missing then it's your job to add it as 
per the specs. Think hard.

3. When you discover a spec issue, architecture gap, or cross-spec inconsistency, 
immediately update the relevant spec in @docs/plan/specs/ with your findings using a 
subagent. When the issue is resolved, update the spec and remove the open question 
using a subagent.

4. When the tests pass, add changed code and any updated specs with "git add" for the 
specific files then do a "git commit" with a message that describes the changes. After 
the commit do a "git push" to push the changes to the remote repository.

999. Important: When authoring documentation (Rust doc comments, spec documentation, 
README files) capture WHY the design decisions were made and what the backing 
implementation or test validates. No orphan docs — every doc must reference real code.

9999. Important: We want single sources of truth, no migrations/adapters/shims. If a 
type or concept is defined in one spec or crate, reference it everywhere else — do not 
redefine it. If tests unrelated to your work fail then it's your job to resolve them as 
part of your increment of change.

99999. Important: The existing TS codebase at @core/ is the reference implementation. 
When implementing Rust equivalents, study the TS code first to understand edge cases 
and real-world behavior. Do not guess — read the source. But do not blindly port — the 
Rust architecture may differ from the TS version per the specs.

999999. When you learn something new about how to build, test, or run any part of SERA, 
update the relevant CLAUDE.md (project root or subdirectory) using a subagent. Keep it 
brief — commands, gotchas, environment quirks only.

9999999. IMPORTANT DO NOT IGNORE: Check @docs/plan/mvs-review-plan.md §6.6 (deferral 
list) before implementing anything. If a feature is marked POST-MVS, do not implement 
it unless explicitly asked. Build MVS first.

99999999. IMPORTANT: Do not implement placeholders, stubs, or todo!() panic 
implementations. Every function you write must be a real, working implementation with 
tests. If you cannot fully implement something, document what's missing as an open 
question in the relevant spec — do not ship dead code.

999999999. For any bugs you discover, resolve them or document them in the relevant 
spec's Open Questions section with a [MVS-BLOCKER] or [DEFERRED] tag using a subagent.

9999999999. SUPER IMPORTANT: Do not place status reports, progress updates, or session 
notes into CLAUDE.md files or specs. CLAUDE.md is for durable learnings (commands, 
gotchas, decisions). Specs are for architecture. Neither is a scratchpad.

99999999999. When specs and code disagree, the spec is the intended target unless the 
code has a good reason (documented in a CLAUDE.md learning or spec open question). If 
you find the spec is wrong based on implementation reality, update the spec.

999999999999. Use parallel subagents for independent work (e.g., reviewing multiple 
specs, implementing independent crates, running searches). Use sequential execution 
for dependent work (e.g., build then test, implement trait then implementor). Only 1 
subagent for build/test operations to avoid conflicts.

9999999999999. The config format uses Kubernetes-style typed manifests (apiVersion, 
kind, metadata, spec) even in single-file mode. See @docs/plan/specs/SPEC-config.md 
§2.4. Do not invent a different config format.

99999999999999. All secrets use reference syntax { secret: "path/to/secret" } resolved 
from SERA_SECRET_<PATH> environment variables. Never hardcode secrets or put them in 
config files. See @docs/plan/specs/SPEC-secrets.md.

999999999999999. Git workflow: commit early, commit often, push at the end of each 
logical unit of work. Commit messages follow conventional commits: 
feat(crate): description, fix(crate): description, docs(specs): description.
```
