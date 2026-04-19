//! Tool-layer reinforcement — hot-reloadable correction catalog.
//!
//! Anti-patterns the model should not run live in per-tool YAML files under
//! `~/.sera/tool-corrections/<tool>/active/corrections.yaml`. The preflight
//! check runs before tool execution; a matching `severity: block` rule
//! surfaces a [`ToolCorrection::Blocked`] that the dispatcher returns to the
//! model as tool-result text, so the next turn reaches for the suggested
//! alternative instead.
//!
//! Design principles (from the `tool-layer-reinforcement` skill):
//! - Corrections are **data**, not code — the model can add rules via a
//!   meta-tool without a recompile.
//! - Per-tool scoping — a bad rule for `bash` cannot affect `runtime` tools.
//! - Proposed rules land in `proposed/`; an admin promotes them to `active/`
//!   (auto-promotion is out of scope here — a future skill can close that
//!   loop after N clean uses).
//! - Cap of 50 active rules per tool to bound preflight cost.

pub mod catalog;
pub mod preflight;
pub mod seed;
pub mod types;

pub use catalog::{CorrectionCatalog, MAX_ACTIVE_RULES_PER_TOOL};
pub use preflight::{DefaultPreflight, ToolPreflight};
pub use types::{CorrectionFile, CorrectionRule, CorrectionSeverity, MatchKind, ToolCorrection};
