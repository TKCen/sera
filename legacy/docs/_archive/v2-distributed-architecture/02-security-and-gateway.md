# 02 Security & The Gateway

The Core Gateway (`sera-core`) is the security sentinel for the entire distributed mesh.

## 1. LLM Proxy Gateway
Agents never connect to LLM providers directly. They use a proxy endpoint.

- **Internal URL**: `http://sera-core:3001/v1/llm/chat/completions`
- **Responsibilities**:
    - **Header Injection**: Injects the `Authorization: Bearer <KEY>` header hidden from the agent.
    - **Audit Logging**: Logs every prompt and response to the Merkle Audit Trail.
    - **Token Metering**: Tracks usage against the agent's assigned quota.
    - **Safety Filtering**: Applies optional pre/post-processing safety guardrails.

## 2. Identity & JWT Auth
When Core spawns an Agent Actor, it generates an **Identity Token**.

- **Format**: JWT (JSON Web Token).
- **Claims**:
    - `agentId`: The unique identifier for the instance.
    - `circleId`: The circle permissions scope.
    - `capabilities`: The list of allowed capability gates (e.g., `internet-access`).
- **Usage**: The agent uses this token to authenticate with the Centrifugo Bus and other internal SERA services.

## 3. Capability-Based Sandboxing
Sandbox permissions are defined in `AGENT.yaml` and enforced by Core at the container level.

| Tier | Isolation Level | Use Case |
| :--- | :--- | :--- |
| **Tier 1 — Isolated** | No network, Read-only FS. | Analysis, sensitive data review. |
| **Tier 2 — Internal** | Access to `sera_net`. | Standard development and automation. |
| **Tier 3 — Executive** | Full Internet access. | Research, web automation, external API calls. |

## 4. Merkle Audit Trail
Every "Mutation Event" (file write, tool call, memory edit) is hashed into a cryptographically linked chain.
- This ensures that if an agent's history is tampered with (deleted or modified), the hash chain will break, alerting the user to a security breach.
