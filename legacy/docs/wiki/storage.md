# SERA Storage Layer

## SQLite Schema (MVS)

SERA uses SQLite (rusqlite) for embedded persistence. The schema covers sessions, transcripts, and audit logging.

### Sessions Table

```sql
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,              -- Unique session ID
    agent_id TEXT NOT NULL,           -- Associated agent name
    session_key TEXT UNIQUE,          -- Queryable key (agent:{agent_id}:main)
    user_id TEXT,                     -- Optional user ID (e.g., Discord user_id)
    state TEXT NOT NULL,              -- active | archived
    created_at TIMESTAMP,             -- Creation time
    archived_at TIMESTAMP,            -- Archival time (null if active)
    metadata TEXT                     -- JSON metadata (context, tags)
);
```

**Session states**: 
- `active` — Currently accepting messages
- `archived` — Previous session, closed for new turns (created on reset)

**Session key format**: `agent:{agent_id}:main`

Example:
- Agent "sera" → session_key = "agent:sera:main"
- Discord channel 123 → session_key = "discord:123"

### Transcript Table

```sql
CREATE TABLE transcript (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    role TEXT NOT NULL,               -- user | assistant | tool | system
    content TEXT,                     -- Message text (null for tool results)
    tool_calls TEXT,                  -- JSON array of tool calls (asst only)
    tool_call_id TEXT,                -- ID matching prior call (tool result only)
    name TEXT,                        -- Tool name (tool results only)
    created_at TIMESTAMP,
    FOREIGN KEY (session_id) REFERENCES sessions(id)
);
```

**Role semantics**:
- `user` — User message (content filled)
- `assistant` — LLM response (content filled, may have tool_calls)
- `tool` — Tool result (tool_call_id, name, content filled)
- `system` — System messages (rare in transcript)

**Tool-related fields**:
- `tool_calls` — JSON serialized `[{ id, function: { name, arguments } }]`
- `tool_call_id` — UUID matching the call in prior assistant message
- `name` — Tool function name

Example transcript flow:

```
id  role       content                  tool_calls  tool_call_id
1   user       "Create a file"          -           -
2   assistant  (null)                   [...]       -       (decided to use file_write)
3   tool       "File created"           -           call_1  (result from file_write)
4   assistant  "File created at ..."    -           -       (final answer)
```

### Session Queue Table

```sql
CREATE TABLE queue (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    message_type TEXT,                -- task | event
    payload TEXT,                     -- JSON
    enqueued_at TIMESTAMP,
    processed_at TIMESTAMP,
    FOREIGN KEY (session_id) REFERENCES sessions(id)
);
```

Simple FIFO queue by session_key for deferred processing (post-MVS).

### Audit Log Table

```sql
CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TIMESTAMP,
    principal_id TEXT,                -- User or service
    action TEXT,                      -- create_session, execute_tool, ...
    resource_id TEXT,                 -- Session ID or tool name
    details TEXT,                     -- JSON
    outcome TEXT                      -- success | failure
);
```

Immutable log of significant events for compliance.

## File-Based Memory

Agents have access to a **markdown-based knowledge base** stored as files in the workspace.

### Structure

```
workspace/
├── memory/
│   ├── index.md              # TOC
│   ├── onboarding.md         # Orientation
│   ├── patterns/
│   │   ├── memory-read.md
│   │   └── shell-execution.md
│   └── learnings/
│       ├── jwt-validation.md
│       └── token-refresh.md
└── .sera/
    └── index.json            # Searchable memory index (vectors, timestamps)
```

### Format

Each file is markdown with **heading-based structure** for keyword search:

```markdown
# Memory: JWT Validation

## Problem
Previously was checking expiry only on API calls, not on session init.

## Solution
Validate exp claim immediately on token parse. Set grace period to 5 seconds.

## Code
```rust
if now_utc > exp_utc - 5 {
    return Err("Token expired");
}
```
```

### Search
Tools like `memory_search` use:
1. **Keyword match** against headings and body
2. **Return heading + context** (N lines before/after match)

Example: `memory_search("token expiry")` returns:
```
## JWT Validation
> Previously was checking expiry only on API calls...
> Validate exp claim immediately...
```

## Session Persistence Workflow

### On User Message
```rust
// 1. Get or create session
let session = db.get_or_create_session(&agent_name)?;

// 2. Append user message
db.append_transcript(&session.id, "user", Some(&message), None, None)?;

// 3. Retrieve recent history for context
let transcript = db.get_transcript_recent(&session.id, 20)?;
```

### During Turn
```rust
// After LLM call: append assistant response
db.append_transcript(&session.id, "assistant", Some(&content), Some(&tool_calls_json), None)?;

// After each tool: append result
db.append_transcript(&session.id, "tool", Some(&result), None, Some(&tool_call_id))?;
```

### On Session Reset
```rust
// 1. Archive current session
db.archive_session(&session.id)?;

// 2. Create new session
let new_session = db.create_session(&agent_name)?;

// New transcript starts fresh
```

## Query Patterns

### Get Recent History
```rust
let rows = db.get_transcript_recent(&session_id, limit: 20)?;
// Returns Vec<TranscriptRow> ordered by created_at DESC, then reversed for LLM order
```

### Find Session by Key
```rust
let session = db.get_session_by_key(&session_key)?;
// Returns Option<SessionRow>
```

### Audit Trail
```rust
let events = db.get_audit_log(&agent_id, since_timestamp)?;
// Filter by principal, action, outcome
```

## Performance Considerations

### Indexes
```sql
CREATE INDEX idx_sessions_agent_id ON sessions(agent_id);
CREATE INDEX idx_transcript_session_id ON transcript(session_id);
CREATE INDEX idx_transcript_created_at ON transcript(created_at);
CREATE INDEX idx_audit_log_timestamp ON audit_log(timestamp);
```

### Transcript Limiting
Always query with `LIMIT` to avoid loading entire conversations. Default: 20 most recent messages.

### Memory Index (Post-MVS)
Vector embeddings stored in `.sera/index.json` for semantic search. MVS uses simple keyword matching only.

## PostgreSQL Support (Post-MVS)

The `sera-db` abstraction supports PostgreSQL via feature flags:

```toml
[features]
sqlite = ["rusqlite"]
postgres = ["sqlx/postgres"]
```

Same schema, different driver (sqlx async instead of rusqlite sync).

---

Last updated: 2026-04-09
