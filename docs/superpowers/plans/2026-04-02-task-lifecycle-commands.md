# Task Lifecycle Commands Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `sera agents` and `sera tasks` commands in the Go CLI for managing agent lifecycle and task queues.

**Architecture:** Refactor shared request logic into a `Client` struct in `cli/client.go`. Implement subcommands in dedicated files using the shared client.

**Tech Stack:** Go (Standard Library), `tabwriter` for formatting.

---

### Task 1: Shared Client Implementation

**Files:**

- Create: `cli/client.go`
- Modify: `cli/auth.go` (extract helpers)

- [ ] **Step 1: Extract helpers from `cli/auth.go`**
      Move `Credentials`, `readCredentials`, `credentialsPath`, `getenv` to a place where they can be shared (or just make them exported if keeping in the same package).

- [ ] **Step 2: Create `cli/client.go`**
      Implement a `SeraClient` struct that handles:
- Base URL from `SERA_API_URL` or default.
- Authorization header from `SERA_API_KEY` or `~/.sera/credentials`.
- JSON request/response handling.
- `ResolveAgentID(nameOrID string) (string, error)` helper.

```go
type SeraClient struct {
    BaseURL string
    APIKey  string
}

func NewClient() (*SeraClient, error) { ... }
func (c *SeraClient) Do(method, path string, body interface{}, result interface{}) error { ... }
func (c *SeraClient) ResolveAgentID(nameOrID string) (string, error) { ... }
```

- [ ] **Step 3: Verify compilation**
      Run: `go build -o sera.exe ./cli`

- [ ] **Step 4: Commit**

```bash
git add cli/auth.go cli/client.go
git commit -m "cli: add shared HTTP client and refactor auth helpers"
```

---

### Task 2: Implement `sera agents` Commands

**Files:**

- Create: `cli/agents.go`
- Modify: `cli/main.go`

- [ ] **Step 1: Implement `runAgents` in `cli/agents.go`**
      Support `list`, `start`, `stop`, `restart`, `logs`.
- `list`: GET `/api/agents/instances`, print table with `tabwriter`.
- `start/stop/restart`: Resolve ID, POST to `/api/agents/instances/:id/start`, etc.
- `logs`: Resolve ID, GET `/api/agents/:id/logs`, print raw text.

- [ ] **Step 2: Wire up in `cli/main.go`**
      Add `agents` case to `main` and update `printUsage`.

- [ ] **Step 3: Verify compilation**
      Run: `go build -o sera.exe ./cli`

- [ ] **Step 4: Commit**

```bash
git add cli/agents.go cli/main.go
git commit -m "cli: implement sera agents commands"
```

---

### Task 3: Implement `sera tasks` Commands

**Files:**

- Create: `cli/tasks.go`
- Modify: `cli/main.go`

- [ ] **Step 1: Implement `runTasks` in `cli/tasks.go`**
      Support `list`, `create`, `get`, `cancel`.
- `list`: Resolve Agent ID, GET `/api/agents/:id/tasks`, print table.
- `create`: Resolve Agent ID, POST `/api/agents/:id/tasks` with `{task: prompt}`.
- `get`: Resolve Agent ID, GET `/api/agents/:id/tasks/:taskId`, print detailed JSON or formatted text.
- `cancel`: Resolve Agent ID, DELETE `/api/agents/:id/tasks/:taskId`.

- [ ] **Step 2: Wire up in `cli/main.go`**
      Add `tasks` case to `main` and update `printUsage`.

- [ ] **Step 3: Verify compilation**
      Run: `go build -o sera.exe ./cli`

- [ ] **Step 4: Commit**

```bash
git add cli/tasks.go cli/main.go
git commit -m "cli: implement sera tasks commands"
```

---

### Task 4: Final Verification & Cleanup

- [ ] **Step 1: Run help to verify usage strings**
      Run: `./sera help`

- [ ] **Step 2: Manual dry-run against localhost:3001 (if running)**
      If core is up, try `sera agents list`.

- [ ] **Step 3: Commit final changes**

```bash
git add .
git commit -m "cli: complete task lifecycle commands implementation"
```
