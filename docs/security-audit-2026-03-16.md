# SERA Security Audit Report
Date: 2026-03-16

## Executive Summary
A comprehensive security audit of the SERA backend, focusing on tier enforcement and sandbox container boundaries, has been completed. The audit identified gaps in tool access validation, missing validation checks for boundary policies, critical path traversal vulnerabilities in builtin filesystem skills, and potential SSRF risks in web operations. This report outlines the vulnerabilities found and the steps taken to mitigate them.

## Findings and Mitigations

### 1. `POST /api/sandbox/exec` Bypassed Tool Validation
**Risk Rating:** High
**Description:** The `/api/sandbox/exec` endpoint permitted container execution commands to bypass the agent's tool access policies defined in `AGENT.yaml` files.
**Mitigation:** `core/src/routes/sandbox.ts` was modified to manually parse `command[0]` as the tool or command name. Policy validation was explicitly added to ensure the tool is not in `manifest.tools.denied` and, if an `allowed` list exists, that the tool is explicitly present in it. If a violation is detected, a `PolicyViolationError` is raised.

### 2. Path Traversal in File Operations (`file-read` & `file-write`)
**Risk Rating:** Critical
**Description:** Builtin skills such as `file-read` and `file-write` accepted unsanitized file paths. Because these builtins ran in the backend process context instead of within isolated sandboxes, agents had the capability to use directory traversal (`../../`) techniques to read or overwrite arbitrary system files.
**Mitigation:** Input sanitation was introduced using `path.resolve` combined with `process.env.WORKSPACE_DIR || process.cwd()`. The resolved paths are strictly verified to confirm they start with the `WORKSPACE_DIR` base path, denying operations pointing outside of the designated workspace safely.

### 3. Server-Side Request Forgery (SSRF) in `web-search`
**Risk Rating:** Medium
**Description:** The built-in `web-search` tool passed user-controlled queries directly to an upstream provider. Without input validation, it allowed direct IP addresses or URL queries, raising the risk of SSRF or open redirect exploits against the backend instance's networking context.
**Mitigation:** Validation was applied in `core/src/skills/builtins/web-search.ts` using a regex test `(/^(https?:\/\/|[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+)/i)` to prohibit explicit direct network protocols, links, and IP queries outright.

### 4. Docker Socket Exposure
**Risk Rating:** High
**Description:** The `SandboxManager` and `DockerVolumeProvider` directly consume the root Docker socket (`/var/run/docker.sock`). If an exploit allows a breakout from the backend process, the attacker could attain elevated system access by conversing directly with the root Docker daemon.
**Recommendations:** Transition away from mounting the root Docker socket into the SERA Core. Two potential mitigations are recommended:
1. **Rootless Docker:** Operate the backend and the Docker daemon in Rootless mode to drop kernel capabilities.
2. **Docker API over TCP with TLS:** Securely expose the Docker API socket using mutually-authenticated TLS, segregating Docker network paths so that the root socket is not directly mapped as a file volume.

### 5. Tier 3 Audit Logging Deficiency
**Risk Rating:** Low
**Description:** Tier 3 agents hold sweeping capabilities (bridge networks, rw mounts) effectively permitting wide operational capabilities. Operations with these containers were logged without an audit warning.
**Mitigation:** An explicit audit trail log flag with a `warning` priority was added to the `spawn` and `exec` methods of the `SandboxManager` for operations performed by agents assigned Tier 3 privileges, assuring they populate appropriately on the logs and audit streams.

### 6. Verification Deficits in Tier Policies
**Risk Rating:** Low
**Description:** While limits were functionally mapped within `TierPolicy.getTierLimits`, test assertions omitted concrete scenarios establishing constraints for each discrete tier.
**Mitigation:** Extended boundary testing was formalized in `TierPolicy.test.ts` via Vitest.

## Conclusion
The boundaries of the SERA Sandbox Manager have been significantly hardened. The applied patches rectify the identified systemic vulnerabilities, safeguarding against directory traversal and unintended executions. Adoption of the Docker Socket mitigations detailed will address remaining systemic risks.