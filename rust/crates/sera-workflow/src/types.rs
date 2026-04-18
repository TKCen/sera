use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A workflow definition — configurable triggered task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    /// Unique name for this workflow.
    pub name: String,
    /// What causes this workflow to fire.
    pub trigger: WorkflowTrigger,
    /// The agent that will execute the workflow turn.
    pub agent_id: String,
    /// Arbitrary configuration passed to the agent.
    pub config: serde_json::Value,
    /// Whether this workflow is active.
    pub enabled: bool,
}

/// The mechanism that causes a workflow to fire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowTrigger {
    /// Fire on a recurring cron schedule.
    Cron(CronSchedule),
    /// Fire when a matching event is published.
    Event(EventPattern),
    /// Fire when a metric crosses a threshold.
    Threshold(ThresholdCondition),
    /// Fire only when explicitly invoked.
    Manual,
}

/// A cron-based schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronSchedule {
    /// Standard cron expression, e.g. `"0 3 * * *"`.
    pub expression: String,
}

/// An event-based trigger pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventPattern {
    /// Optional filter on `EventKind` (serialised string form).
    pub kind: Option<String>,
    /// Optional filter on `EventSource` (serialised string form).
    pub source: Option<String>,
    /// Key/value pairs that must be present in the event metadata.
    #[serde(default)]
    pub metadata_match: HashMap<String, serde_json::Value>,
}

/// A metric-threshold trigger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdCondition {
    /// The metric name to watch (e.g. `"memory_count"`).
    pub metric: String,
    /// Comparison operator.
    pub operator: ThresholdOperator,
    /// Threshold value.
    pub value: f64,
    /// Scope to a specific agent; `None` means global.
    pub agent_id: Option<String>,
}

/// Comparison operators for threshold conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThresholdOperator {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
}
