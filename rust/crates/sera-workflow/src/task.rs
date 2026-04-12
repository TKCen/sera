use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use sera_types::evolution::{BlastRadius, ChangeArtifactId};

/// Content-addressed identifier for a [`WorkflowTask`] — SHA-256 of canonical fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WorkflowTaskId {
    pub hash: [u8; 32],
}

impl WorkflowTaskId {
    /// Compute from the canonical pipe-delimited content string.
    pub fn from_content(
        title: &str,
        description: &str,
        first_acceptance_criterion: &str,
        source_formula: &str,
        source_location: &str,
        created_at: DateTime<Utc>,
    ) -> Self {
        let content = format!(
            "{title}|{description}|{first_acceptance_criterion}|{source_formula}|{source_location}|{}",
            created_at.to_rfc3339()
        );
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        Self { hash }
    }
}

impl fmt::Display for WorkflowTaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.hash))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowTaskIdParseError;

impl fmt::Display for WorkflowTaskIdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid WorkflowTaskId: expected 64 hex characters")
    }
}

impl std::error::Error for WorkflowTaskIdParseError {}

impl FromStr for WorkflowTaskId {
    type Err = WorkflowTaskIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s).map_err(|_| WorkflowTaskIdParseError)?;
        let hash: [u8; 32] = bytes.try_into().map_err(|_| WorkflowTaskIdParseError)?;
        Ok(Self { hash })
    }
}

/// Status of a workflow task — mirrors the beads Issue status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTaskStatus {
    Open,
    InProgress,
    /// Atomically reserved; transitions to InProgress on confirm.
    Hooked,
    Blocked,
    Deferred,
    Closed,
    Pinned,
}

/// Broad category of a workflow task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTaskType {
    Feature,
    Bug,
    Chore,
    Research,
    Meta,
    Dream,
}

/// The external thing a task is waiting for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwaitType {
    GhRun,
    GhPr,
    Timer,
    Human,
    Mail,
    Change,
}

/// Semantic relationship between two tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyType {
    Blocks,
    Related,
    ParentChild,
    DiscoveredFrom,
    ConditionalBlocks,
}

/// A directed dependency edge between two tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowTaskDependency {
    pub from: WorkflowTaskId,
    pub to: WorkflowTaskId,
    pub kind: DependencyType,
}

/// Named sentinel positions in a workflow sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowSentinel {
    Start,
    SelfLoop,
    Prev,
    Next,
    End,
    Named(String),
}

/// A single work item in the SERA workflow system — mirrors the beads Issue schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTask {
    pub id: WorkflowTaskId,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,

    pub status: WorkflowTaskStatus,
    /// 0 = highest priority.
    pub priority: u8,
    pub task_type: WorkflowTaskType,

    pub assignee: Option<String>,
    pub due_at: Option<DateTime<Utc>>,
    pub defer_until: Option<DateTime<Utc>>,

    pub metadata: serde_json::Value,

    pub await_type: Option<AwaitType>,
    pub await_id: Option<String>,
    #[serde(
        serialize_with = "serialize_duration_opt",
        deserialize_with = "deserialize_duration_opt"
    )]
    pub timeout: Option<std::time::Duration>,

    /// If true, this task is discarded once Closed.
    pub ephemeral: bool,

    pub source_formula: Option<String>,
    pub source_location: Option<String>,

    pub created_at: DateTime<Utc>,

    /// §4.6 obligation — blast-radius scope for meta/change tasks.
    pub meta_scope: Option<BlastRadius>,
    /// §4.6 obligation — linked change artifact.
    pub change_artifact_id: Option<ChangeArtifactId>,

    pub dependencies: Vec<WorkflowTaskDependency>,
}

// Serde helpers for std::time::Duration (stored as seconds u64).

fn serialize_duration_opt<S>(dur: &Option<std::time::Duration>, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match dur {
        Some(d) => s.serialize_some(&d.as_secs()),
        None => s.serialize_none(),
    }
}

fn deserialize_duration_opt<'de, D>(
    d: D,
) -> Result<Option<std::time::Duration>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<u64> = Option::deserialize(d)?;
    Ok(opt.map(std::time::Duration::from_secs))
}

impl WorkflowTask {
    /// Construct a new task, computing its content-addressed [`WorkflowTaskId`].
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        title: impl Into<String>,
        description: impl Into<String>,
        acceptance_criteria: Vec<String>,
        status: WorkflowTaskStatus,
        priority: u8,
        task_type: WorkflowTaskType,
        source_formula: Option<String>,
        source_location: Option<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        let title = title.into();
        let description = description.into();
        let first_ac = acceptance_criteria.first().map(String::as_str).unwrap_or("");
        let id = WorkflowTaskId::from_content(
            &title,
            &description,
            first_ac,
            source_formula.as_deref().unwrap_or(""),
            source_location.as_deref().unwrap_or(""),
            created_at,
        );
        Self {
            id,
            title,
            description,
            acceptance_criteria,
            status,
            priority,
            task_type,
            assignee: None,
            due_at: None,
            defer_until: None,
            metadata: serde_json::Value::Null,
            await_type: None,
            await_id: None,
            timeout: None,
            ephemeral: false,
            source_formula,
            source_location,
            created_at,
            meta_scope: None,
            change_artifact_id: None,
            dependencies: Vec::new(),
        }
    }
}
