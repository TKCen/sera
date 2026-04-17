use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use sera_hitl::ApprovalId;
use sera_types::evolution::{BlastRadius, ChangeArtifactId};

/// Workflow-local identifier for a GitHub Actions workflow run.
///
/// Intentionally a newtype in `sera-workflow` (not `sera-types`) — the id is a
/// scheduler-side handle used only for [`AwaitType::GhRun`] gate lookups. Real
/// GitHub run ids are `u64`, but we carry the string form so the scheduler
/// doesn't couple to `octocrab`'s numeric type and so opaque synthetic ids
/// (e.g. from tests or dry-run fixtures) round-trip cleanly.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GhRunId(pub String);

impl GhRunId {
    /// Construct a [`GhRunId`] from anything that can be turned into a `String`.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the underlying id as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GhRunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Terminal / non-terminal status of a GitHub Actions workflow run.
///
/// Mirrors the GitHub API `status` + `conclusion` surface, collapsed into a
/// single enum the ready-queue cares about. The scheduler only needs a
/// binary signal (terminal vs not), so non-terminal transient states
/// (`Queued`, `InProgress`) and the catch-all [`GhRunStatus::Unknown`] map to
/// not-ready; every terminal state maps to ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GhRunStatus {
    /// Run is queued but not yet executing.
    Queued,
    /// Run is currently executing.
    InProgress,
    /// Run finished successfully.
    Completed,
    /// Run finished with a failure.
    Failed,
    /// Run was cancelled before completion.
    Cancelled,
    /// Run was skipped (e.g. conditional `if:` false).
    Skipped,
    /// Run completed with a neutral conclusion (neither success nor failure).
    Neutral,
    /// Status cannot be determined from the GitHub API response — treated as
    /// not-ready so the scheduler falls back to conservative behaviour.
    Unknown,
}

impl GhRunStatus {
    /// Returns `true` iff the run has reached a terminal state the scheduler
    /// should treat as "resolved". The workflow proceeds on any terminal
    /// conclusion — the downstream handler branches on success/failure itself,
    /// mirroring the [`AwaitType::Human`] contract.
    ///
    /// `Unknown` is deliberately non-terminal: we prefer to keep the task
    /// pending over waking it on an ambiguous signal.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Skipped | Self::Neutral
        )
    }
}

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

/// Workflow-local identifier for a GitHub pull request.
///
/// Mirrors [`GhRunId`]: a newtype over `String` so the scheduler doesn't
/// couple to a numeric GitHub PR number type and opaque synthetic ids
/// (e.g. from tests or fixtures) round-trip cleanly.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GhPrId(pub String);

impl GhPrId {
    /// Construct a [`GhPrId`] from anything that can be turned into a `String`.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the underlying id as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GhPrId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Workflow-local identifier for an email thread.
///
/// Intentionally a newtype in `sera-workflow` (not `sera-types`) — the id is a
/// scheduler-side handle used only for [`AwaitType::Mail`] gate lookups.
/// Thread ids are opaque strings; implementations may map to RFC 2822
/// Message-IDs, provider-specific thread handles, or synthetic test fixtures.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MailThreadId(pub String);

impl MailThreadId {
    /// Construct a [`MailThreadId`] from anything that can be turned into a `String`.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the underlying id as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MailThreadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Observable event state for an email thread gate ([`AwaitType::Mail`]).
///
/// The scheduler only needs a binary signal (terminal vs not). Non-terminal
/// states ([`MailEvent::Pending`]) and the conservative catch-all
/// [`MailEvent::Unknown`] map to not-ready; every terminal state maps to ready.
///
/// Design A (MVS): gate resolves when the identified thread has received at
/// least one reply since task creation, or when the thread is administratively
/// closed. Pattern-based inbox-wide matching is deferred to a post-MVS
/// refinement.
///
// TODO(post-MVS): consider AwaitType::Mail { pattern: MailPattern } for richer
// inbox-wide event matching (Design B). File a follow-up bead before removing
// this comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailEvent {
    /// Thread exists but no reply has arrived yet (non-terminal).
    Pending,
    /// At least one reply has been received on the thread (terminal).
    ReplyReceived,
    /// Thread was administratively closed (terminal — no further replies
    /// expected; the task must wake up so its handler can record the closure).
    Closed,
    /// Event state cannot be determined from the mail backend — treated as
    /// not-ready so the scheduler falls back to conservative behaviour.
    Unknown,
}

impl MailEvent {
    /// Returns `true` iff this event represents a terminal state.
    ///
    /// [`MailEvent::ReplyReceived`] is terminal because a reply is the primary
    /// expected resolution of a mail gate — the task should wake up to process
    /// the incoming message.
    ///
    /// [`MailEvent::Closed`] is terminal because waiting indefinitely on a
    /// closed thread would strand the task; the handler can branch on
    /// [`MailEvent::Closed`] vs [`MailEvent::ReplyReceived`] after waking.
    ///
    /// [`MailEvent::Pending`] and [`MailEvent::Unknown`] are non-terminal:
    /// `Pending` means we are still waiting, and `Unknown` is deliberately
    /// conservative — we never self-satisfy on an ambiguous signal.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::ReplyReceived | Self::Closed)
    }
}

/// Terminal / non-terminal state of a GitHub pull request.
///
/// The scheduler only needs a binary signal (terminal vs not). Non-terminal
/// transient states ([`GhPrState::Open`], [`GhPrState::Draft`]) and the
/// conservative catch-all [`GhPrState::Unknown`] map to not-ready; every
/// terminal state maps to ready.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GhPrState {
    /// PR is open and ready for review (non-terminal).
    Open,
    /// PR has been closed without merging (terminal).
    Closed,
    /// PR has been merged (terminal).
    Merged,
    /// PR is still being drafted (non-terminal — still being worked on).
    Draft,
    /// State cannot be determined from the GitHub API response — treated as
    /// not-ready so the scheduler falls back to conservative behaviour.
    Unknown,
}

impl GhPrState {
    /// Returns `true` iff the PR has reached a terminal state the scheduler
    /// should treat as "resolved". The workflow proceeds on any terminal
    /// conclusion — the downstream handler branches on the outcome itself.
    ///
    /// `Draft` is deliberately non-terminal (PR still being worked on).
    /// `Unknown` is deliberately non-terminal (conservative — don't wake on
    /// ambiguous lookup).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Closed | Self::Merged)
    }
}

/// The external thing a task is waiting for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwaitType {
    /// GitHub Actions workflow-run gate — task is not ready until the run
    /// referenced by `run_id` reaches a terminal [`GhRunStatus`]
    /// (Completed/Failed/Cancelled/Skipped/Neutral) via a
    /// [`GhRunLookup`](crate::ready::GhRunLookup) during scheduling.
    ///
    /// `repo` is carried alongside the id purely for operator debugging and
    /// audit trails — the gate logic itself keys only on `run_id`.
    GhRun {
        run_id: GhRunId,
        repo: String,
    },
    /// GitHub pull-request gate — task is not ready until the PR referenced
    /// by `pr_id` reaches a terminal [`GhPrState`] (Closed / Merged) via a
    /// [`GhPrLookup`](crate::ready::GhPrLookup) during scheduling.
    ///
    /// `repo` is carried purely for operator debugging and audit trails —
    /// the gate logic itself keys only on `pr_id`.
    GhPr {
        pr_id: GhPrId,
        repo: String,
    },
    /// Time-based gate — task is not ready until `now >= not_before`.
    ///
    /// This gate is pull-based: the ready-queue asks
    /// [`is_timer_ready`](crate::ready::is_timer_ready) during scheduling.
    /// No background timer wheel is needed.
    Timer {
        not_before: DateTime<Utc>,
    },
    /// Human-in-the-loop gate — task is not ready until the referenced
    /// [`ApprovalId`] resolves to a terminal [`TicketStatus`]
    /// (Approved / Rejected / Expired) in sera-hitl.
    ///
    /// Pull-based integration: the ready-queue polls via a
    /// [`HitlLookup`](crate::ready::HitlLookup) during scheduling.
    /// Workflows proceed regardless of the terminal outcome — callers branch
    /// on the ticket status themselves after the task is claimed.
    Human {
        approval_id: ApprovalId,
    },
    /// Inbound-email gate — task is not ready until the thread referenced by
    /// `thread_id` receives a reply or is administratively closed. The event
    /// is surfaced via a [`MailLookup`](crate::ready::MailLookup) during
    /// scheduling.
    ///
    /// Pull-based integration: the ready-queue polls `thread_event` and
    /// resolves when [`MailEvent::is_terminal`] returns `true`.
    /// Workflows proceed on both [`MailEvent::ReplyReceived`] and
    /// [`MailEvent::Closed`] — the downstream handler branches on the outcome.
    Mail {
        thread_id: MailThreadId,
    },
    /// Change-artifact gate — task is not ready until the change artifact
    /// referenced by `artifact_id` reaches a terminal [`ChangeState`]
    /// (Applied / Rejected / Failed / Superseded) via a
    /// [`ChangeLookup`](crate::ready::ChangeLookup) during scheduling.
    ///
    /// The id is hash-derived and self-identifying — no `repo` field is needed.
    Change {
        artifact_id: ChangeArtifactId,
    },
}

/// Terminal / non-terminal state of a SERA change artifact.
///
/// Mirrors the `ChangeArtifactStatus` surface from sera-meta, collapsed into a
/// single enum the ready-queue cares about. Non-terminal states
/// ([`ChangeState::Proposed`], [`ChangeState::UnderReview`],
/// [`ChangeState::Approved`]) and the conservative catch-all
/// [`ChangeState::Unknown`] map to not-ready; every terminal state maps to ready.
///
/// [`ChangeState::Approved`] is deliberately **non-terminal**: an approved
/// change may still be queued for apply — the task must not wake until the
/// apply itself completes (or fails).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeState {
    /// Change has been proposed but not yet reviewed (non-terminal).
    Proposed,
    /// Change is actively under review (non-terminal).
    UnderReview,
    /// Change has been approved but not yet applied (non-terminal — apply still
    /// pending).
    Approved,
    /// Change was successfully applied (terminal).
    Applied,
    /// Change was rejected during review (terminal).
    Rejected,
    /// Apply attempt failed (terminal).
    Failed,
    /// Change was superseded by a newer artifact (terminal).
    Superseded,
    /// State cannot be determined — treated as not-ready (conservative).
    Unknown,
}

impl ChangeState {
    /// Returns `true` iff the change has reached a terminal state the scheduler
    /// should treat as "resolved". The workflow proceeds on any terminal
    /// conclusion — the downstream handler branches on the outcome itself.
    ///
    /// `Approved` is deliberately non-terminal: the apply step has not run yet.
    /// `Unknown` is deliberately non-terminal: conservative — don't wake on
    /// ambiguous lookup.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Applied | Self::Rejected | Self::Failed | Self::Superseded)
    }
}

impl AwaitType {
    /// Returns true iff this await gate is a Timer whose `not_before` has
    /// elapsed relative to `now` (inclusive boundary — `not_before == now`
    /// counts as ready).
    ///
    /// Returns false for non-Timer variants — they use other gates
    /// (GhRun/GhPr/Human/Mail/Change) not yet implemented.
    pub fn is_timer_ready(&self, now: DateTime<Utc>) -> bool {
        match self {
            AwaitType::Timer { not_before } => now >= *not_before,
            _ => false,
        }
    }
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

/// Input for constructing a new [`WorkflowTask`].
pub struct WorkflowTaskInput {
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub status: WorkflowTaskStatus,
    pub priority: u8,
    pub task_type: WorkflowTaskType,
    pub source_formula: Option<String>,
    pub source_location: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl WorkflowTask {
    /// Construct a new task, computing its content-addressed [`WorkflowTaskId`].
    pub fn new(input: WorkflowTaskInput) -> Self {
        let first_ac = input.acceptance_criteria.first().map(String::as_str).unwrap_or("");
        let id = WorkflowTaskId::from_content(
            &input.title,
            &input.description,
            first_ac,
            input.source_formula.as_deref().unwrap_or(""),
            input.source_location.as_deref().unwrap_or(""),
            input.created_at,
        );
        Self {
            id,
            title: input.title,
            description: input.description,
            acceptance_criteria: input.acceptance_criteria,
            status: input.status,
            priority: input.priority,
            task_type: input.task_type,
            assignee: None,
            due_at: None,
            defer_until: None,
            metadata: serde_json::Value::Null,
            await_type: None,
            await_id: None,
            timeout: None,
            ephemeral: false,
            source_formula: input.source_formula,
            source_location: input.source_location,
            created_at: input.created_at,
            meta_scope: None,
            change_artifact_id: None,
            dependencies: Vec::new(),
        }
    }
}
