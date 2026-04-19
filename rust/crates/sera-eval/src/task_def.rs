//! Task definition, task result, and metric types.
//!
//! These types are the shared vocabulary across the eval harness: adapters
//! produce [`TaskDef`]s, the runner consumes them and produces [`TaskResult`]s,
//! and the store persists [`TaskResult`]s for later reporting.
//!
//! Task files are YAML frontmatter + markdown body (see
//! `docs/sera-eval-design.md` §5). The rationale body is parsed as free text
//! so humans can include reasoning without the schema churning.

use serde::{Deserialize, Serialize};

use crate::EvalError;

/// A single evaluation task. Parsed from a YAML frontmatter file plus an
/// optional markdown body (`rationale`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDef {
    /// Stable identifier, e.g. `sera-internal-0001`. Must be unique within
    /// a suite + version.
    pub id: String,
    /// One-line title for reports.
    pub title: String,
    /// Which suite this task belongs to (`sera-internal`, `swe-bench-lite`, …).
    pub suite: String,
    /// Bump when a task's semantics change so historical results stay pinned
    /// to the version they ran against.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Free-form tags for filtering (`memory`, `cross-session`, …).
    #[serde(default)]
    pub tags: Vec<String>,

    /// Pre-task fixture state (seeded memory, skills to load, sandbox tier).
    #[serde(default)]
    pub setup: TaskSetup,

    /// What the agent actually sees when the task begins.
    pub input: TaskInput,

    /// How we grade the final response / sandbox state.
    pub expected: ExpectedOutcome,

    /// Optional set of gold memory segment ids used to compute memory P@k.
    #[serde(default)]
    pub gold_memory_segment_ids: Vec<String>,

    /// Per-task budget. The runner enforces these as hard stops.
    #[serde(default)]
    pub budget: TaskBudget,

    /// Markdown body from the task file (rationale + human context). Not
    /// used for grading; carried so it shows up in reports.
    #[serde(default)]
    pub rationale: String,
}

fn default_version() -> u32 {
    1
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskSetup {
    #[serde(default)]
    pub memory_seed: Vec<MemorySeedItem>,
    #[serde(default)]
    pub skills: Vec<String>,
    /// Sandbox tier cap (1 = local-only, 2 = curated net, 3 = open net).
    /// Defaults to tier 1 — the safest default.
    #[serde(default = "default_sandbox_tier")]
    pub sandbox_tier: u8,
}

fn default_sandbox_tier() -> u8 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySeedItem {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInput {
    /// The initial user prompt. Most tasks have exactly one; multi-turn
    /// tasks can extend this with `follow_ups` in a later schema version.
    pub prompt: String,
    #[serde(default)]
    pub follow_ups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedOutcome {
    /// Assertions evaluated against the agent's final state. All must pass
    /// for a `Pass` verdict.
    #[serde(default)]
    pub assertions: Vec<Assertion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assertion {
    pub kind: AssertionKind,
    #[serde(default)]
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionKind {
    ContainsAny,
    ContainsAll,
    NotContains,
    Regex,
    ToolCalled,
    FileWritten,
    PatchApplies,
    ExternalGrader,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskBudget {
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_max_wall_seconds")]
    pub max_wall_seconds: u32,
}

impl Default for TaskBudget {
    fn default() -> Self {
        Self {
            max_turns: default_max_turns(),
            max_tokens: default_max_tokens(),
            max_wall_seconds: default_max_wall_seconds(),
        }
    }
}

fn default_max_turns() -> u32 {
    10
}
fn default_max_tokens() -> u32 {
    8_000
}
fn default_max_wall_seconds() -> u32 {
    300
}

/// Outcome verdict for a single task run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Fail,
    Error,
    Skipped,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
            Verdict::Error => "error",
            Verdict::Skipped => "skipped",
        }
    }
}

/// Full metric bag recorded per task run. The runner stores this verbatim as
/// JSON in `eval_task_results.metrics_json`; the summary columns on the row
/// are denormalised copies to make SQL reporting cheap.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricSet {
    pub turns: u32,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub latency_ms: u64,
    pub tool_calls_total: u32,
    pub tool_calls_valid: u32,
    /// Only set for `+memory` / `+full` harness profiles when the task has
    /// `gold_memory_segment_ids`. `None` means "not measured for this run".
    pub memory_precision: Option<MemoryPrecision>,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPrecision {
    pub k: u32,
    pub gold_count: u32,
    pub hit_count: u32,
}

impl MemoryPrecision {
    pub fn precision_at_k(&self) -> f64 {
        if self.k == 0 {
            0.0
        } else {
            f64::from(self.hit_count) / f64::from(self.k)
        }
    }
}

/// Result of running one task once. Serialised into `eval_task_results`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub verdict: Verdict,
    pub metrics: MetricSet,
    /// Full turn-by-turn transcript. Opaque JSON so the shape can evolve
    /// without a schema change — the store always writes it as a string.
    #[serde(default)]
    pub transcript: serde_json::Value,
    pub error_message: Option<String>,
}

/// Parse a task file — YAML frontmatter (`---`-delimited) followed by an
/// optional markdown body that populates [`TaskDef::rationale`].
///
/// A file with no `---` delimiters is treated as pure YAML with no rationale.
pub fn parse_task_file(contents: &str) -> Result<TaskDef, EvalError> {
    let (frontmatter, rationale) = split_frontmatter(contents);
    let mut def: TaskDef = serde_yaml::from_str(frontmatter)?;
    if !rationale.is_empty() {
        def.rationale = rationale.to_string();
    }
    validate_task(&def)?;
    Ok(def)
}

fn split_frontmatter(contents: &str) -> (&str, &str) {
    let trimmed = contents.trim_start();
    if let Some(rest) = trimmed.strip_prefix("---") {
        // Accept either `\n` or `\r\n` after the opening marker.
        let rest = rest.trim_start_matches(['\r', '\n']);
        if let Some(end) = rest.find("\n---") {
            let frontmatter = &rest[..end];
            let body = rest[end + 4..].trim_start_matches(['\r', '\n']).trim();
            return (frontmatter, body);
        }
    }
    (contents, "")
}

fn validate_task(def: &TaskDef) -> Result<(), EvalError> {
    if def.id.trim().is_empty() {
        return Err(EvalError::TaskDefInvalid("id must not be empty".into()));
    }
    if def.suite.trim().is_empty() {
        return Err(EvalError::TaskDefInvalid("suite must not be empty".into()));
    }
    if def.input.prompt.trim().is_empty() {
        return Err(EvalError::TaskDefInvalid(
            "input.prompt must not be empty".into(),
        ));
    }
    for a in &def.expected.assertions {
        if matches!(
            a.kind,
            AssertionKind::ContainsAny
                | AssertionKind::ContainsAll
                | AssertionKind::NotContains
                | AssertionKind::Regex
                | AssertionKind::ToolCalled
                | AssertionKind::ExternalGrader
        ) && a.values.is_empty()
        {
            return Err(EvalError::TaskDefInvalid(format!(
                "assertion {:?} requires at least one value",
                a.kind
            )));
        }
    }
    Ok(())
}
