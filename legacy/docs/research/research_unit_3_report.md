# Technical Audit Report: Security & Validation Layer

## 1. Sandbox Enforcement Mechanisms

The system employs a multi-layered approach to isolation, combining OS-level containerization with protocol-level permission boundaries.

### Docker Isolation (Container Boundary)
The `DockerEnvironment` class (`tools/environments/docker.py`) implements a hardened execution environment:
- **Capability Dropping**: Uses `--cap-drop ALL` and selectively adds only essential capabilities (`DAC_OVERRIDE`, `CHOWN`, `FOWNER`). This minimizes the attack surface within the container.
- **Privilege Escalation Prevention**: Enforces `--security-opt no-new-privileges`, preventing processes from gaining new privileges via setuid/setgid binaries.
- **Resource Constraints**: Implements PID limits (`--pids-limit 256`) and configurable CPU/Memory/Disk quotas to prevent DoS attacks (e.g., fork bombs).
- **Filesystem Hardening**: Uses `tmpfs` for `/tmp`, `/var/tmp`, and `/run` with `nosuid` and `noexec` flags where appropriate, limiting the ability to execute malicious binaries from writable scratch space.

### Egress & Network Control
While a dedicated "egress-proxy" was not explicitly identified in the codebase, network isolation is managed via:
- **Network Mode Configuration**: The `DockerEnvironment` allows for `--network=none`, providing complete network isolation for sensitive tasks.

- **Credential Isolation**: Credentials (OAuth tokens, etc.) are mounted into containers as **read-only** (`:ro`) volumes, ensuring the agent can authenticate but cannot tamper with host-side credentials.

### Permission Boundaries (ACP Bridge)
The `acp_adapter/permissions.py` acts as a critical security bridge between the Agent and the external Controller (via ACP):
- **Permission Mapping**: Maps high-level ACP `PermissionOptionKind` (e.g., `allow_once`, `allow_always`) to internal Hermes execution modes (`once`, `always`, `deny`).
- **Timeout Enforcement**: Implements a strict 60-second timeout on permission requests, preventing the agent from hanging indefinitely if a user or controller fails to respond.

## 2. E2E Testing Strategy (Playwright/Pytest)

The testing strategy focuses on validating the full orchestration loop through integration-style tests in the `tests/` directory:
- **Plugin Lifecycle**: `tests/agent/test_memory_plugin_e2e.py` validates that external plugins (like the SQLite provider) can be dynamically registered, integrated into the tool surface, and correctly handle stateful conversation turns.
- **Protocol Integrity**: Tests like `tests/acp/test_mcp_e2e.py` ensure that the Model Context Protocol (MCP) integration correctly updates the agent's tool definitions without breaking existing capabilities.
- **Setup Verification**: `tests/hermes_cli/test_setup_matrix_e2ee.py` validates the security of the initial environment setup and encryption.

## 3. Prevention of Unauthorized Access & Lateral Movement

The system prevents lateral movement between agents and unauthorized tool access through:
- **Toolset Filtering**: The `AIAgent` supports `enabled_toolsets` and `disabled_toolsets`. This allows administrators to restrict an agent's capabilities at instantiation, preventing it from even "seeing" dangerous tools.
- **Command Guarding**: The `terminal_tool.py` integrates with a `check_dangerous_command` mechanism (via `tools/approval.py`). This intercepts high-risk shell commands and routes them through the ACP approval callback for human intervention.

- **Process Isolation**: Each agent session is tied to a unique `session_id`. In Docker mode, each task can be assigned its own container instance with separate volumes, preventing one agent from accessing the workspace of another.

## 4. Identified Gaps in Validation/Testing Coverage

1.  **Egress Proxy Verification**: There is no explicit test suite validating that network-restricted containers actually block unauthorized outbound connections (e.g., testing `network=none` enforcement).
2.  **Privilege Escalation Regression**: While `--no-new-privileges` is set, there are no automated "exploit" tests (e.g., attempting to run a setuid binary) to ensure this flag remains effective during future Docker engine or configuration updates.
3.  **Cross-Container Leakage**: There is a lack of testing for filesystem leakage between different `task_id` instances when using persistent bind mounts.
4.  **Toolset Boundary Testing**: While toolsets can be disabled, there are no E2E tests that specifically attempt to "force" a disabled tool call to verify the enforcement logic in `run_agent.py`.
