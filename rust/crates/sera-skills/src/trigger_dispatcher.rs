//! Trigger dispatcher — matches [`SkillTrigger`] variants and keyword
//! triggers against turn content to decide which skills should fire.
//!
//! Two sources of triggers are considered:
//!
//! 1. `SkillConfig::trigger` ([`SkillTrigger`]):
//!    - [`SkillTrigger::Always`] — fires unconditionally on any non-empty turn.
//!    - [`SkillTrigger::Event(pat)`] — fires when `pat` occurs in the turn
//!      content (case-insensitive substring).
//!    - [`SkillTrigger::Manual`] — never auto-fires; activation is explicit.
//!
//! 2. `SkillDefinition::triggers` (the frontmatter keyword list): each
//!    keyword is a case-insensitive substring match against the turn content.
//!
//! This is a deliberately minimal implementation ("basic — not the full
//! learning loop"): no scoring, no priority, no ranking; just a yes/no
//! decision per skill with the matching reason attached for observability.
//!
//! # Example
//!
//! ```
//! use sera_skills::trigger_dispatcher::{TriggerDispatcher, MatchReason};
//! use sera_types::skill::{SkillConfig, SkillDefinition, SkillMode, SkillTrigger};
//!
//! let cfg = SkillConfig {
//!     name: "code-review".into(),
//!     version: "1.0.0".into(),
//!     description: "review".into(),
//!     mode: SkillMode::OnDemand,
//!     trigger: SkillTrigger::Event("review".into()),
//!     tools: vec![],
//!     context_injection: None,
//!     config: serde_json::json!({}),
//! };
//! let mut d = TriggerDispatcher::new();
//! d.register(cfg, None);
//! let hits = d.dispatch("please review this PR");
//! assert_eq!(hits.len(), 1);
//! assert_eq!(hits[0].name, "code-review");
//! assert!(matches!(hits[0].reason, MatchReason::Event(_)));
//! ```

use sera_types::skill::{SkillConfig, SkillDefinition, SkillError, SkillRegistry, SkillTrigger};

/// Why a skill matched the turn content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchReason {
    /// Matched via [`SkillTrigger::Always`].
    Always,
    /// Matched via [`SkillTrigger::Event`] with the given pattern.
    Event(String),
    /// Matched via a keyword in `SkillDefinition::triggers`.
    Keyword(String),
}

/// A single skill that matched the turn content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMatch {
    pub name: String,
    pub reason: MatchReason,
}

/// Registered dispatcher entry: the config (for the trigger variant) plus the
/// optional full definition (for frontmatter keyword triggers).
#[derive(Debug, Clone)]
struct Entry {
    config: SkillConfig,
    definition: Option<SkillDefinition>,
}

/// Matches [`SkillTrigger`] and keyword triggers against turn content.
#[derive(Debug, Default)]
pub struct TriggerDispatcher {
    entries: Vec<Entry>,
}

impl TriggerDispatcher {
    /// Construct an empty dispatcher.
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Register a skill. If the skill is already registered (by name), the
    /// previous entry is replaced so hot-reload is a no-op apart from swap.
    pub fn register(&mut self, config: SkillConfig, definition: Option<SkillDefinition>) {
        self.entries.retain(|e| e.config.name != config.name);
        self.entries.push(Entry { config, definition });
    }

    /// Remove a previously registered skill.
    pub fn unregister(&mut self, name: &str) {
        self.entries.retain(|e| e.config.name != name);
    }

    /// Number of registered skills.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no skills are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return all skills whose triggers match the turn content.
    ///
    /// The returned list is deduplicated by skill name; the first matching
    /// reason wins. Empty `content` produces an empty result even for
    /// `SkillTrigger::Always` to avoid firing on startup or idle ticks.
    pub fn dispatch(&self, content: &str) -> Vec<SkillMatch> {
        if content.trim().is_empty() {
            return Vec::new();
        }
        let haystack = content.to_lowercase();
        let mut hits: Vec<SkillMatch> = Vec::new();

        for entry in &self.entries {
            if hits.iter().any(|h| h.name == entry.config.name) {
                continue;
            }

            if let Some(reason) = match_entry(&haystack, entry) {
                hits.push(SkillMatch {
                    name: entry.config.name.clone(),
                    reason,
                });
            }
        }

        hits
    }

    /// Dispatch and activate matched skills against a [`SkillRegistry`].
    ///
    /// Skills already active are skipped (they report `AlreadyActive` which
    /// is not a dispatcher-level failure). Returns the matches that were
    /// newly activated, preserving their [`MatchReason`].
    pub fn fire(
        &self,
        content: &str,
        registry: &mut SkillRegistry,
    ) -> Vec<SkillMatch> {
        let mut fired = Vec::new();
        for m in self.dispatch(content) {
            match registry.activate(&m.name) {
                Ok(_) => fired.push(m),
                Err(SkillError::AlreadyActive(_)) => {
                    // Skill already on — not an error; not counted as newly fired.
                }
                Err(e) => {
                    tracing::debug!(
                        skill = %m.name,
                        error = %e,
                        "trigger dispatcher: activation skipped"
                    );
                }
            }
        }
        fired
    }
}

/// Attempt to match a single entry's triggers against the (lowercased) content.
///
/// Order of precedence within an entry:
/// 1. `SkillConfig::trigger` (Always / Event / Manual)
/// 2. `SkillDefinition::triggers` keyword list
fn match_entry(haystack_lower: &str, entry: &Entry) -> Option<MatchReason> {
    match &entry.config.trigger {
        SkillTrigger::Always => return Some(MatchReason::Always),
        SkillTrigger::Event(pat) => {
            let needle = pat.to_lowercase();
            if !needle.is_empty() && haystack_lower.contains(&needle) {
                return Some(MatchReason::Event(pat.clone()));
            }
        }
        SkillTrigger::Manual => {
            // Fall through — keyword triggers may still match.
        }
    }

    if let Some(def) = &entry.definition {
        for kw in &def.triggers {
            let needle = kw.to_lowercase();
            if !needle.is_empty() && haystack_lower.contains(&needle) {
                return Some(MatchReason::Keyword(kw.clone()));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::skill::{SkillMode, SkillRegistry};

    fn cfg(name: &str, trigger: SkillTrigger) -> SkillConfig {
        SkillConfig {
            name: name.into(),
            version: "1.0.0".into(),
            description: "test".into(),
            mode: SkillMode::OnDemand,
            trigger,
            tools: vec![],
            context_injection: None,
            config: serde_json::json!({}),
        }
    }

    fn def_with_triggers(name: &str, triggers: Vec<&str>) -> SkillDefinition {
        SkillDefinition {
            name: name.into(),
            description: None,
            version: None,
            parameters: None,
            source: None,
            body: None,
            triggers: triggers.into_iter().map(String::from).collect(),
            model_override: None,
            context_budget_tokens: None,
            tool_bindings: vec![],
            mcp_servers: vec![],
        }
    }

    #[test]
    fn always_fires_on_any_nonempty_content() {
        let mut d = TriggerDispatcher::new();
        d.register(cfg("a", SkillTrigger::Always), None);
        let hits = d.dispatch("anything goes");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].reason, MatchReason::Always);
    }

    #[test]
    fn always_does_not_fire_on_empty_content() {
        let mut d = TriggerDispatcher::new();
        d.register(cfg("a", SkillTrigger::Always), None);
        assert!(d.dispatch("").is_empty());
        assert!(d.dispatch("   ").is_empty());
    }

    #[test]
    fn manual_never_fires_without_keywords() {
        let mut d = TriggerDispatcher::new();
        d.register(cfg("m", SkillTrigger::Manual), None);
        assert!(d.dispatch("please activate m").is_empty());
    }

    #[test]
    fn event_fires_case_insensitive() {
        let mut d = TriggerDispatcher::new();
        d.register(cfg("cr", SkillTrigger::Event("Review".into())), None);
        let hits = d.dispatch("Please REVIEW this patch");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].reason, MatchReason::Event("Review".into()));
    }

    #[test]
    fn event_does_not_fire_without_match() {
        let mut d = TriggerDispatcher::new();
        d.register(cfg("cr", SkillTrigger::Event("security".into())), None);
        assert!(d.dispatch("just a regular conversation").is_empty());
    }

    #[test]
    fn empty_event_pattern_never_matches() {
        let mut d = TriggerDispatcher::new();
        d.register(cfg("e", SkillTrigger::Event(String::new())), None);
        assert!(d.dispatch("anything").is_empty());
    }

    #[test]
    fn keyword_triggers_match_when_config_is_manual() {
        let mut d = TriggerDispatcher::new();
        d.register(
            cfg("k", SkillTrigger::Manual),
            Some(def_with_triggers("k", vec!["audit", "review"])),
        );
        let hits = d.dispatch("Please audit the code");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].reason, MatchReason::Keyword("audit".into()));
    }

    #[test]
    fn config_trigger_takes_precedence_over_keywords() {
        let mut d = TriggerDispatcher::new();
        d.register(
            cfg("k", SkillTrigger::Event("deploy".into())),
            Some(def_with_triggers("k", vec!["audit"])),
        );
        // Content matches Event, not Keyword — Event wins because it's checked first.
        let hits = d.dispatch("please deploy and audit");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].reason, MatchReason::Event("deploy".into()));
    }

    #[test]
    fn keyword_fires_when_event_does_not_match() {
        let mut d = TriggerDispatcher::new();
        d.register(
            cfg("k", SkillTrigger::Event("deploy".into())),
            Some(def_with_triggers("k", vec!["audit"])),
        );
        let hits = d.dispatch("time for an audit");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].reason, MatchReason::Keyword("audit".into()));
    }

    #[test]
    fn multiple_skills_independent() {
        let mut d = TriggerDispatcher::new();
        d.register(cfg("a", SkillTrigger::Event("alpha".into())), None);
        d.register(cfg("b", SkillTrigger::Event("beta".into())), None);
        d.register(cfg("c", SkillTrigger::Manual), None);
        let hits = d.dispatch("alpha and beta walked in");
        assert_eq!(hits.len(), 2);
        let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
    }

    #[test]
    fn register_replaces_existing_entry() {
        let mut d = TriggerDispatcher::new();
        d.register(cfg("s", SkillTrigger::Event("old".into())), None);
        d.register(cfg("s", SkillTrigger::Event("new".into())), None);
        assert_eq!(d.len(), 1);
        assert!(d.dispatch("talk about old things").is_empty());
        let hits = d.dispatch("talk about new things");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].reason, MatchReason::Event("new".into()));
    }

    #[test]
    fn unregister_removes_entry() {
        let mut d = TriggerDispatcher::new();
        d.register(cfg("s", SkillTrigger::Always), None);
        assert_eq!(d.len(), 1);
        d.unregister("s");
        assert!(d.is_empty());
        assert!(d.dispatch("anything").is_empty());
    }

    #[test]
    fn fire_activates_matched_skills_in_registry() {
        let mut d = TriggerDispatcher::new();
        let c = cfg("k", SkillTrigger::Event("go".into()));
        d.register(c.clone(), None);

        let mut reg = SkillRegistry::new();
        reg.register(c);

        let fired = d.fire("let's go now", &mut reg);
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].name, "k");
        assert_eq!(reg.active_skills().len(), 1);
    }

    #[test]
    fn fire_skips_already_active_skills() {
        let mut d = TriggerDispatcher::new();
        let c = cfg("k", SkillTrigger::Always);
        d.register(c.clone(), None);

        let mut reg = SkillRegistry::new();
        reg.register(c);
        reg.activate("k").unwrap();

        let fired = d.fire("anything", &mut reg);
        assert!(fired.is_empty(), "already-active skills must not be reported as newly fired");
        assert_eq!(reg.active_skills().len(), 1);
    }

    #[test]
    fn fire_ignores_unregistered_in_registry() {
        // Skill is registered with the dispatcher but not with the registry —
        // activation fails silently and nothing is fired.
        let mut d = TriggerDispatcher::new();
        d.register(cfg("orphan", SkillTrigger::Always), None);

        let mut reg = SkillRegistry::new();
        let fired = d.fire("anything", &mut reg);
        assert!(fired.is_empty());
        assert!(reg.active_skills().is_empty());
    }

    #[test]
    fn dispatch_is_deduplicated_by_name() {
        // Defensive: if the same name is pushed directly (bypassing register),
        // dispatch still returns the first hit only. We test via register which
        // replaces — registering twice should not cause double-fire.
        let mut d = TriggerDispatcher::new();
        d.register(cfg("s", SkillTrigger::Always), None);
        d.register(cfg("s", SkillTrigger::Always), None);
        let hits = d.dispatch("hi");
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn keyword_match_is_case_insensitive() {
        let mut d = TriggerDispatcher::new();
        d.register(
            cfg("k", SkillTrigger::Manual),
            Some(def_with_triggers("k", vec!["Audit"])),
        );
        let hits = d.dispatch("do an AUDIT now");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].reason, MatchReason::Keyword("Audit".into()));
    }

    #[test]
    fn empty_keyword_is_ignored() {
        let mut d = TriggerDispatcher::new();
        // Empty keyword in the definition must not match the empty string in
        // every haystack — otherwise every turn fires the skill.
        let mut def = def_with_triggers("k", vec![]);
        def.triggers.push(String::new());
        d.register(cfg("k", SkillTrigger::Manual), Some(def));
        assert!(d.dispatch("anything at all").is_empty());
    }
}
