# Agent Templates & Instances

## The Two-Tier Model

SERA separates agent _blueprints_ from _deployed instances_. This keeps reusable definitions separate from runtime configuration.

| Concept           | Analogy              | Purpose                                              |
| ----------------- | -------------------- | ---------------------------------------------------- |
| **AgentTemplate** | Class / Docker image | Reusable blueprint defining defaults                 |
| **Agent**         | Instance / Container | Named deployment with its own identity and overrides |

### Why This Matters

- Templates are **community-publishable** — like Helm charts for agents
- Instances have their own **identity, memory namespace, and audit trail**
- Configuration can **evolve post-instantiation** without touching the template
- **Override resolution is explicit** — template spec merged with instance overrides

## Built-in Templates

SERA ships with four built-in templates:

### Sera (Primary Agent)

The entry point for the entire system. Auto-instantiated on first boot.

- **Role:** Primary resident agent and orchestrator
- **Capabilities:** Agent management, circle coordination, scheduling, channel setup
- **Tools:** knowledge-store, knowledge-query, web-search, shell-exec, schedule-task, delegate-task
- **Subagents:** Can spawn researcher (5), developer (2), architect (1)
- **Scheduled activities:** Reflection, knowledge consolidation, curiosity research, goal review

### Developer

Full-stack development agent for coding tasks.

- **Role:** Senior software engineer
- **Tools:** file-read, file-write, shell-exec, knowledge-query
- **Subagents:** Can spawn researcher (2)

### Architect

System design and review agent.

- **Role:** Distributed systems architect
- **Tools:** file-read, file-write, knowledge-store, knowledge-query, web-search (no shell)
- **Subagents:** Can spawn researcher (3), browser (1)

### Researcher

Lightweight investigative agent, typically ephemeral.

- **Role:** Research, synthesis, and analysis
- **Sandbox:** tier-3 (most restrictive)
- **Tools:** web-search, knowledge-store, knowledge-query, file-read, file-write (no shell)

## Override Mechanics

Instance overrides follow specific merge rules:

| Override syntax    | Effect                                              |
| ------------------ | --------------------------------------------------- |
| Direct replacement | `model.name: "new-model"` replaces template default |
| `$append`          | Adds items to an existing array (e.g., skills)      |
| `$remove`          | Removes items from an inherited array               |
| Absent field       | Inherits from template                              |

```yaml
# Instance overrides
overrides:
  model:
    name: qwen2.5-coder-32b # replaces template model
  skills:
    $append:
      - agentic-coding-v1 # adds to template's skill list
  resources:
    maxLlmTokensPerHour: 200000 # replaces template budget
```

## Lifecycle Modes

Every agent has a `lifecycle.mode`:

**Persistent agents** survive restarts, have their own DB record, memory namespace, and are visible in the UI. They can be edited post-creation.

**Ephemeral agents** exist only during a task. They are spawned by parent agents via the `spawn-subagent` tool and auto-removed on completion. They cannot create persistent agents — this is a hard guard against privilege escalation.

## Subagent Spawning

Templates declare which subagent types an agent can spawn:

```yaml
subagents:
  allowed:
    - templateRef: researcher
      maxInstances: 3
      lifecycle: ephemeral
      requiresApproval: false
    - templateRef: developer
      maxInstances: 2
      lifecycle: ephemeral
      requiresApproval: true # operator must approve
```

**Capability inheritance:** A subagent cannot exceed its parent's resolved capabilities. The parent cannot grant more than it has.
