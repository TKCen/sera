# `sera-workflow` — Workflow Engine for SERA

**Crate:** `rust/crates/sera-workflow`
**Type:** library
**Spec:** `docs/plan/specs/SPEC-workflow-engine.md`
**Types source:** `rust/crates/sera-workflow/src/types.rs`

---

## Overview

`sera-workflow` implements SERA's workflow engine providing automated, configurable task orchestration. It provides:

- **`WorkflowDef`** — workflow definitions with triggers (Cron/Event/Threshold/Manual)
- **`WorkflowRegistry`** — registration and querying of active workflows
- **`WorkflowTask`** — task management with content-addressed IDs and dependency tracking
- **`DreamingConfig`** — built-in dreaming workflow with Light/REM/Deep sleep phases
- **`ClaimProtocol`** — atomic task claiming with stale claim reaping
- **`ScheduleService`** — cron schedule validation and next-fire computation
- **`TerminationTriad`** — termination condition checking

This crate handles workflow definition, triggering, task creation, dependency resolution, and execution coordination. It provides the scheduling backbone for SERA's autonomous operation.

---

## Architecture: Workflow Engine Design

### Workflow Lifecycle

```
  ┌─────────────────────────────────────────────────────────────┐
  │  Workflow Definition Phase                                  │
  │                                                             │
  │  WorkflowDef { name, trigger, agent_id, config, enabled }  │
  │  ↓                                                          │
  │  WorkflowRegistry::register(workflow_def)                   │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Trigger Evaluation Phase                                   │
  │                                                             │
  │  ┌─ Cron: schedule::next_fire(cron_expr) → DateTime         │
  │  ├─ Event: EventPattern::matches(incoming_event) → bool     │
  │  ├─ Threshold: metric_value op threshold_value → bool       │
  │  └─ Manual: explicit invocation only                       │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Task Generation Phase                                      │
  │                                                             │
  │  WorkflowTask {                                             │
  │    id: WorkflowTaskId::from_content(...),                   │
  │    title, description, acceptance_criteria,                 │
  │    dependencies: Vec<WorkflowTaskDependency>,               │
  │    status: Created,                                         │
  │    agent_id, config                                         │
  │  }                                                          │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Dependency Resolution & Task Readiness                     │
  │                                                             │
  │  ready::ready_tasks(all_tasks) → Vec<WorkflowTaskId>        │
  │  ready::dependency_closure(task_id) → Vec<WorkflowTaskId>   │
  │                                                             │
  │  Task is ready when:                                        │
  │  - status == Created                                        │
  │  - All dependencies satisfied (Completed/AwaitCompletion)   │
  │  - No blocking claim exists                                 │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Atomic Claiming & Execution                                │
  │                                                             │
  │  claim::claim_task(task_id) → ClaimToken                    │
  │  ↓                                                          │
  │  Agent executes task                                        │
  │  ↓                                                          │
  │  claim::confirm_claim(token) → task.status = Completed     │
  │                                                             │
  │  StaleClaimReaper::reap_stale() (background)                │
  └─────────────────────────────────────────────────────────────┘
```

### Dreaming Workflow Architecture

```
  ┌─────────────────────────────────────────────────────────────┐
  │  Light Sleep Phase — Recent Memory Scan                     │
  │                                                             │
  │  - Scan last N days for recently accessed memories         │
  │  - Surface high-frequency, high-recency candidates         │
  │  - Quick pattern detection                                  │
  │                                                             │
  │  LightSleepConfig { lookback_days, limit }                 │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  REM Sleep Phase — Pattern Detection                       │
  │                                                             │
  │  - Deep pattern analysis across memory corpus              │
  │  - Cross-reference themes and concepts                     │
  │  - Identify conceptual relationships                       │
  │                                                             │
  │  RemSleepConfig { lookback_days, min_pattern_strength }    │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Deep Sleep Phase — Memory Consolidation                   │
  │                                                             │
  │  DreamCandidate {                                          │
  │    memory_key, scores, total_score,                        │
  │    recall_count, unique_queries                            │
  │  }                                                         │
  │                                                             │
  │  Scoring: relevance(0.30) + frequency(0.24) +             │
  │          query_diversity(0.15) + recency(0.15) +          │
  │          consolidation(0.10) + conceptual_richness(0.06)   │
  │                                                             │
  │  Promote candidates above DeepSleepConfig.min_score        │
  └─────────────────────────────────────────────────────────────┘
```

---

## Core Types

### `WorkflowDef`

A workflow definition that specifies when and how a workflow should execute.

```rust
pub struct WorkflowDef {
    pub name: String,                    // Unique workflow identifier
    pub trigger: WorkflowTrigger,        // What causes this workflow to fire
    pub agent_id: String,                // Agent that executes the workflow
    pub config: serde_json::Value,       // Configuration passed to agent
    pub enabled: bool,                   // Whether workflow is active
}
```

### `WorkflowTrigger`

The mechanism that determines when a workflow fires.

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowTrigger {
    Cron(CronSchedule),           // Time-based recurring schedule
    Event(EventPattern),          // Event-driven trigger
    Threshold(ThresholdCondition), // Metric threshold trigger
    Manual,                       // Explicit invocation only
}
```

**Cron Triggers:**
```rust
CronSchedule { expression: "0 3 * * *" }  // Daily at 3 AM
```

**Event Triggers:**
```rust
EventPattern {
    kind: Some("memory_updated".to_string()),
    source: Some("sera-memory".to_string()),
    metadata_match: {
        "agent_id": "sera-analyst",
        "tier": 2
    }
}
```

**Threshold Triggers:**
```rust
ThresholdCondition {
    metric: "memory_count".to_string(),
    operator: ThresholdOperator::Gt,
    value: 1000.0,
    agent_id: Some("sera-analyst".to_string())  // None for global
}
```

### `WorkflowTask`

A concrete task instance generated from a workflow execution.

```rust
pub struct WorkflowTask {
    pub id: WorkflowTaskId,                           // Content-addressed hash
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub dependencies: Vec<WorkflowTaskDependency>,
    pub status: WorkflowTaskStatus,
    pub agent_id: String,
    pub config: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub blast_radius: Option<BlastRadius>,
    pub change_artifact_id: Option<ChangeArtifactId>,
}
```

**Task Dependencies:**
```rust
pub struct WorkflowTaskDependency {
    pub task_id: WorkflowTaskId,
    pub dependency_type: DependencyType,
    pub await_type: AwaitType,
}

pub enum DependencyType {
    BlocksOn,      // Hard dependency — must wait
    EnhancedBy,    // Soft dependency — can proceed without
}

pub enum AwaitType {
    Completion,    // Wait for task to be fully completed
    Availability,  // Wait for task to be claimable/started
}
```

**Task Status Lifecycle:**
```rust
pub enum WorkflowTaskStatus {
    Created,       // Initial state, eligible for claiming
    Claimed,       // Claimed by an agent, work in progress
    Completed,     // Successfully completed
    Failed,        // Failed execution
    Cancelled,     // Explicitly cancelled
}
```

### `WorkflowTaskId`

Content-addressed identifier using SHA-256 of canonical task fields.

```rust
WorkflowTaskId::from_content(
    title, 
    description, 
    first_acceptance_criterion,
    source_formula,
    source_location,
    created_at
) -> WorkflowTaskId
```

Ensures deterministic task identification and deduplication.

---

## Dreaming Configuration

### `DreamingConfig`

Top-level configuration for the built-in dreaming workflow.

```rust
pub struct DreamingConfig {
    pub enabled: bool,
    pub schedule: String,         // Cron expression, e.g. "0 2 * * *" 
    pub phases: DreamingPhases,
}

pub struct DreamingPhases {
    pub light: LightSleepConfig,
    pub rem: RemSleepConfig, 
    pub deep: DeepSleepConfig,
}
```

### Phase Configurations

**Light Sleep — Recent Memory Scan:**
```rust
pub struct LightSleepConfig {
    pub lookback_days: u32,    // Days back to scan (default: 7)
    pub limit: u32,            // Max candidates (default: 100)
}
```

**REM Sleep — Pattern Detection:**
```rust
pub struct RemSleepConfig {
    pub lookback_days: u32,         // Days back for patterns (default: 30)
    pub min_pattern_strength: f64,  // Min strength score (default: 0.7)
}
```

**Deep Sleep — Memory Consolidation:**
```rust
pub struct DeepSleepConfig {
    pub min_score: f64,            // Min composite score (default: 0.8)
    pub min_recall_count: u32,     // Min recall count (default: 3)
    pub min_unique_queries: u32,   // Min unique queries (default: 2)
    pub max_age_days: u32,         // Max age in days (default: 365)
    pub limit: u32,                // Max promotions per run (default: 10)
}
```

### `DreamCandidate`

A memory candidate being evaluated for promotion during deep sleep.

```rust
pub struct DreamCandidate {
    pub memory_key: String,
    pub scores: HashMap<String, f64>,  // Individual dimension scores
    pub total_score: f64,              // Weighted composite score
    pub recall_count: u32,
    pub unique_queries: u32,
}

impl DreamCandidate {
    pub fn compute_score(&mut self, weights: &DreamingWeights) {
        // Weighted sum using default weights that sum to 1.0:
        // relevance(0.30) + frequency(0.24) + query_diversity(0.15) +
        // recency(0.15) + consolidation(0.10) + conceptual_richness(0.06)
    }
}
```

---

## Task Management API

### Ready Task Resolution

```rust
// Find all tasks ready for execution
pub fn ready_tasks(tasks: &[WorkflowTask]) -> Vec<WorkflowTaskId>

// Get all dependencies that must be satisfied for a task
pub fn dependency_closure(
    task_id: WorkflowTaskId, 
    tasks: &[WorkflowTask]
) -> Vec<WorkflowTaskId>
```

### Atomic Task Claiming

```rust
pub async fn claim_task(task_id: WorkflowTaskId) -> Result<ClaimToken, ClaimError>

pub async fn confirm_claim(token: ClaimToken) -> Result<(), ClaimError>

pub struct ClaimToken {
    pub task_id: WorkflowTaskId,
    pub claimed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub agent_id: String,
}
```

**Claim Protocol:**
1. `claim_task()` atomically sets task status to `Claimed` and returns a `ClaimToken`
2. Agent executes the task
3. `confirm_claim()` atomically sets task status to `Completed` using the token
4. If agent crashes, `StaleClaimReaper` eventually resets stale claims back to `Created`

### Stale Claim Reaping

```rust
pub struct StaleClaimReaper {
    claim_timeout: Duration,  // How long claims are valid (default: 1 hour)
}

impl StaleClaimReaper {
    pub async fn reap_stale(&self) -> Result<Vec<WorkflowTaskId>, ClaimError>
}
```

---

## Termination Detection

### `TerminationConfig`

Configuration for workflow termination conditions.

```rust
pub struct TerminationConfig {
    pub max_tasks: Option<u32>,
    pub max_duration: Option<Duration>,
    pub completion_percentage: Option<f64>,
    pub custom_conditions: Vec<String>,
}

pub fn check_termination(
    config: &TerminationConfig,
    state: &TerminationState
) -> Option<TerminationReason>
```

### Termination Reasons

```rust
pub enum TerminationReason {
    MaxTasksReached,
    MaxDurationExceeded, 
    CompletionThresholdMet,
    CustomCondition(String),
    AllTasksCompleted,
}
```

---

## Schedule Service

### Cron Schedule Validation

```rust
pub fn validate_cron(expression: &str) -> Result<(), ScheduleError>

pub fn next_fire(expression: &str, after: DateTime<Utc>) -> Result<DateTime<Utc>, ScheduleError>

pub fn fire_times(
    expression: &str, 
    start: DateTime<Utc>, 
    end: DateTime<Utc>
) -> Result<Vec<DateTime<Utc>>, ScheduleError>
```

Supports standard 5-field cron expressions:
```
* * * * *
┬ ┬ ┬ ┬ ┬
│ │ │ │ └─── day of week (0-7, Sunday = 0 or 7)
│ │ │ └───── month (1-12)  
│ │ └─────── day of month (1-31)
│ └───────── hour (0-23)
└─────────── minute (0-59)
```

---

## Workflow Registry

### `WorkflowRegistry`

**Note: This type is marked as deprecated** in favor of the newer Phase 0 task management approach.

```rust
#[deprecated = "Use Phase 0 task management instead"]
pub struct WorkflowRegistry {
    workflows: HashMap<String, WorkflowDef>,
}

impl WorkflowRegistry {
    pub fn new() -> Self
    pub fn register(&mut self, workflow: WorkflowDef)
    pub fn get(&self, name: &str) -> Option<&WorkflowDef>
    pub fn list(&self) -> Vec<&WorkflowDef>
    pub fn remove(&mut self, name: &str) -> bool
}
```

---

## Error Handling

### `WorkflowError`

```rust
pub enum WorkflowError {
    ScheduleError { expression: String, reason: String },
    TaskNotFound { task_id: WorkflowTaskId },
    ClaimError { task_id: WorkflowTaskId, reason: String },
    DependencyError { task_id: WorkflowTaskId, missing_deps: Vec<WorkflowTaskId> },
    InvalidConfiguration { field: String, reason: String },
    DatabaseError { source: Box<dyn std::error::Error + Send + Sync> },
}
```

---

## Usage Examples

### Basic Workflow Definition

```rust
use sera_workflow::{WorkflowDef, WorkflowTrigger, CronSchedule};

let workflow = WorkflowDef {
    name: "daily-memory-cleanup".to_string(),
    trigger: WorkflowTrigger::Cron(CronSchedule {
        expression: "0 2 * * *".to_string(),  // 2 AM daily
    }),
    agent_id: "sera-janitor".to_string(),
    config: serde_json::json!({
        "target_tier": 1,
        "max_age_days": 90,
        "dry_run": false
    }),
    enabled: true,
};
```

### Event-Driven Workflow

```rust
use sera_workflow::{WorkflowTrigger, EventPattern};
use std::collections::HashMap;

let mut metadata_match = HashMap::new();
metadata_match.insert("priority".to_string(), serde_json::json!("high"));

let workflow = WorkflowDef {
    name: "high-priority-response".to_string(),
    trigger: WorkflowTrigger::Event(EventPattern {
        kind: Some("message_received".to_string()),
        source: Some("discord".to_string()),
        metadata_match,
    }),
    agent_id: "sera-responder".to_string(),
    config: serde_json::json!({
        "response_template": "urgent",
        "escalation_enabled": true
    }),
    enabled: true,
};
```

### Task Creation and Claiming

```rust
use sera_workflow::{WorkflowTask, WorkflowTaskId, WorkflowTaskStatus, claim_task, confirm_claim};
use chrono::Utc;

// Create a task
let task = WorkflowTask {
    id: WorkflowTaskId::from_content(
        "Analyze recent conversations",
        "Review and summarize conversation patterns from the last week",
        "Conversation patterns identified and documented",
        "weekly_analysis_workflow",
        "sera-analyst",
        Utc::now(),
    ),
    title: "Analyze recent conversations".to_string(),
    description: "Review and summarize conversation patterns from the last week".to_string(),
    acceptance_criteria: vec![
        "Conversation patterns identified and documented".to_string(),
        "Summary report generated".to_string(),
    ],
    dependencies: vec![],
    status: WorkflowTaskStatus::Created,
    agent_id: "sera-analyst".to_string(),
    config: serde_json::json!({
        "lookback_days": 7,
        "include_metadata": true
    }),
    created_at: Utc::now(),
    claimed_at: None,
    completed_at: None,
    blast_radius: None,
    change_artifact_id: None,
};

// Claim and execute
let token = claim_task(task.id).await?;
// ... agent executes task ...
confirm_claim(token).await?;
```

### Dreaming Configuration

```rust
use sera_workflow::{
    DreamingConfig, DreamingPhases, LightSleepConfig, 
    RemSleepConfig, DeepSleepConfig
};

let dreaming_config = DreamingConfig {
    enabled: true,
    schedule: "0 3 * * *".to_string(),  // 3 AM daily
    phases: DreamingPhases {
        light: LightSleepConfig {
            lookback_days: 7,
            limit: 100,
        },
        rem: RemSleepConfig {
            lookback_days: 30,
            min_pattern_strength: 0.7,
        },
        deep: DeepSleepConfig {
            min_score: 0.8,
            min_recall_count: 3,
            min_unique_queries: 2,
            max_age_days: 365,
            limit: 10,
        },
    },
};
```

---

## Integration Points

### With `sera-events`

Workflows subscribe to events via `EventPattern` triggers and can publish workflow lifecycle events.

### With `sera-queue`

Tasks are enqueued for agent execution after successful claiming.

### With `sera-db`

Workflow definitions, tasks, and claims are persisted in PostgreSQL with ACID guarantees.

### With `sera-auth`

Task execution respects agent authorization and capability policies.

---

## Public API Surface

```rust
// Core workflow types
pub use types::{
    WorkflowDef, WorkflowTrigger, CronSchedule, EventPattern, 
    ThresholdCondition, ThresholdOperator
};

// Task management
pub use task::{
    WorkflowTask, WorkflowTaskId, WorkflowTaskStatus, WorkflowTaskType,
    WorkflowTaskDependency, DependencyType, AwaitType, WorkflowSentinel
};

// Dreaming workflow
pub use dreaming::{
    DreamingConfig, DreamingPhases, LightSleepConfig, RemSleepConfig,
    DeepSleepConfig, DreamingWeights, DreamCandidate
};

// Atomic claiming
pub use claim::{claim_task, confirm_claim, ClaimToken, ClaimError, StaleClaimReaper};

// Task readiness
pub use ready::{ready_tasks, dependency_closure};

// Termination detection
pub use termination::{
    check_termination, TerminationConfig, TerminationReason, 
    TerminationState, WorkflowTermination
};

// Schedule utilities
pub use schedule::{validate_cron, next_fire, fire_times, ScheduleError};

// Session key generation
pub use session_key::workflow_session_key;

// Legacy registry (deprecated)
#[allow(deprecated)]
pub use registry::WorkflowRegistry;

// Error types
pub use error::WorkflowError;
```

---

## Test Coverage

The test suite in `src/tests.rs` covers:

- **Workflow trigger evaluation**: Cron schedule parsing, event pattern matching, threshold evaluation
- **Task lifecycle**: Creation, claiming, execution, completion, failure
- **Dependency resolution**: Ready task detection, dependency closure computation
- **Claim protocol**: Atomic claiming, stale claim reaping, timeout handling
- **Dreaming workflow**: Phase configuration, candidate scoring, promotion logic
- **Schedule service**: Cron validation, next fire calculation, timezone handling
- **Termination detection**: Various termination conditions and edge cases
- **Content addressing**: Task ID generation and collision detection
- **Error conditions**: Invalid configurations, missing dependencies, claim conflicts

Integration tests verify workflow orchestration end-to-end with mock agents and database backends.