//! LLM-visible tool schema filter (bead sera-hwny).
//!
//! The gateway forwards the agent manifest's `tools.allow` glob list as
//! `SERA_AGENT_TOOLS_ALLOW` (and reserves `SERA_AGENT_TOOLS_DENY` for an
//! operator override / future manifest field). This module narrows the set
//! of `ToolDefinition`s the runtime hands to the LLM so the model only sees
//! the tools the manifest authorises.
//!
//! This is **disclosure control** only — it does not gate execution.
//! `CapabilityRegistry` (loaded via `SERA_AGENT_POLICY_REF`) and `ToolPolicy`
//! still own the AuthZ decision at dispatch time. Filtering here just
//! prevents the LLM from being steered toward tools it isn't allowed to use.

use crate::types::ToolDefinition;

const ENV_ALLOW: &str = "SERA_AGENT_TOOLS_ALLOW";
const ENV_DENY: &str = "SERA_AGENT_TOOLS_DENY";

/// Allow / deny glob lists applied to tool names before they're handed to the LLM.
#[derive(Debug, Default, Clone)]
pub struct ToolNameFilter {
    allow: Vec<String>,
    deny: Vec<String>,
}

impl ToolNameFilter {
    /// Read the filter from `SERA_AGENT_TOOLS_ALLOW` and `SERA_AGENT_TOOLS_DENY`
    /// (both comma-separated globs). Empty / unset env vars yield empty lists.
    pub fn from_env() -> Self {
        Self {
            allow: parse_env_list(ENV_ALLOW),
            deny: parse_env_list(ENV_DENY),
        }
    }

    /// Build a filter from explicit glob lists (test entry point).
    pub fn from_globs(allow: Vec<String>, deny: Vec<String>) -> Self {
        Self { allow, deny }
    }

    /// `true` when no allow or deny patterns are set — the filter is a no-op
    /// and `definitions()` returns its input untouched.
    pub fn is_pass_through(&self) -> bool {
        self.allow.is_empty() && self.deny.is_empty()
    }

    /// Decide whether `name` should be visible to the LLM.
    ///
    /// Rules (deny overrides allow):
    /// 1. Any deny match → hidden.
    /// 2. If allow is non-empty and no allow pattern matches → hidden.
    /// 3. Otherwise → visible.
    pub fn matches(&self, name: &str) -> bool {
        if self.deny.iter().any(|p| glob_match(p, name)) {
            return false;
        }
        if !self.allow.is_empty() && !self.allow.iter().any(|p| glob_match(p, name)) {
            return false;
        }
        true
    }

    /// Filter a `ToolDefinition` list in place semantics (returns a new Vec).
    pub fn filter_definitions(&self, defs: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
        if self.is_pass_through() {
            return defs;
        }
        defs.into_iter()
            .filter(|d| self.matches(&d.function.name))
            .collect()
    }
}

fn parse_env_list(var: &str) -> Vec<String> {
    std::env::var(var)
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Match `name` against a simple `*`-wildcard glob (`*`, prefix `*`, suffix
/// `*`, internal `*`, or literal). Greedy left-to-right segment search; no
/// `?`, `[]`, or escape support — the manifest only uses `*` (e.g. `memory_*`).
fn glob_match(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == name;
    }

    let segments: Vec<&str> = pattern.split('*').collect();
    let leading_star = pattern.starts_with('*');
    let trailing_star = pattern.ends_with('*');

    let mut cursor = 0usize;
    for (i, seg) in segments.iter().enumerate() {
        if seg.is_empty() {
            continue;
        }
        let is_first = i == 0;
        let is_last = i == segments.len() - 1;

        if is_first && !leading_star {
            if !name[cursor..].starts_with(seg) {
                return false;
            }
            cursor += seg.len();
            continue;
        }

        if is_last && !trailing_star {
            return name[cursor..].ends_with(seg) && name.len() - cursor >= seg.len();
        }

        match name[cursor..].find(seg) {
            Some(idx) => cursor += idx + seg.len(),
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionDefinition, ToolDefinition};

    fn def(name: &str) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: name.to_string(),
                description: String::new(),
                parameters: serde_json::Value::Null,
            },
        }
    }

    // ── glob_match unit tests ────────────────────────────────────────────────

    #[test]
    fn glob_exact_match() {
        assert!(glob_match("file-read", "file-read"));
        assert!(!glob_match("file-read", "file-write"));
    }

    #[test]
    fn glob_universal_star() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn glob_prefix_wildcard() {
        assert!(glob_match("memory_*", "memory_search"));
        assert!(glob_match("memory_*", "memory_"));
        assert!(!glob_match("memory_*", "file-read"));
        assert!(!glob_match("memory_*", "mem"));
    }

    #[test]
    fn glob_suffix_wildcard() {
        assert!(glob_match("*-read", "file-read"));
        assert!(glob_match("*-read", "knowledge-read"));
        assert!(!glob_match("*-read", "file-write"));
    }

    #[test]
    fn glob_internal_wildcard() {
        assert!(glob_match("file-*-x", "file-foo-x"));
        assert!(!glob_match("file-*-x", "file-foo"));
    }

    // ── ToolNameFilter behaviour ─────────────────────────────────────────────

    #[test]
    fn filter_pass_through_when_empty() {
        let f = ToolNameFilter::default();
        assert!(f.is_pass_through());
        assert!(f.matches("anything"));
        let defs = vec![def("a"), def("b")];
        assert_eq!(f.filter_definitions(defs).len(), 2);
    }

    #[test]
    fn filter_allow_only_keeps_matching() {
        let f = ToolNameFilter::from_globs(vec!["memory_*".into()], vec![]);
        assert!(f.matches("memory_search"));
        assert!(f.matches("memory_recall"));
        assert!(!f.matches("file-read"));
    }

    #[test]
    fn filter_deny_overrides_allow() {
        // allow: memory_*, deny: memory_dangerous → matching deny still hides it.
        let f =
            ToolNameFilter::from_globs(vec!["memory_*".into()], vec!["memory_dangerous".into()]);
        assert!(f.matches("memory_search"));
        assert!(!f.matches("memory_dangerous"));
    }

    #[test]
    fn filter_deny_without_allow_blocks_only_deny_hits() {
        // No allow list = allow everything; deny still removes specific tools.
        let f = ToolNameFilter::from_globs(vec![], vec!["shell-*".into()]);
        assert!(f.matches("file-read"));
        assert!(!f.matches("shell-exec"));
    }

    #[test]
    fn filter_definitions_with_restricted_allow_yields_only_matching() {
        // Acceptance criterion 1: tools.allow=["memory_*"] → only memory_*
        // entries reach the LLM-visible tool_defs slice.
        let f = ToolNameFilter::from_globs(vec!["memory_*".into()], vec![]);
        let defs = vec![
            def("file-read"),
            def("file-write"),
            def("memory_search"),
            def("memory_recall"),
            def("shell-exec"),
        ];
        let filtered = f.filter_definitions(defs);
        let names: Vec<_> = filtered.iter().map(|d| d.function.name.as_str()).collect();
        assert_eq!(names, vec!["memory_search", "memory_recall"]);
    }

    #[test]
    fn filter_definitions_pass_through_preserves_order_and_count() {
        let f = ToolNameFilter::default();
        let defs = vec![def("a"), def("b"), def("c")];
        let filtered = f.filter_definitions(defs);
        let names: Vec<_> = filtered.iter().map(|d| d.function.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_env_list_handles_empty_and_whitespace() {
        // Test the parser directly via a wrapper: inject through a temp env var.
        // Note: env vars are process-global; pick a name unlikely to collide.
        let key = "SERA_HWNY_FILTER_TEST_LIST";
        // empty
        unsafe { std::env::set_var(key, "") };
        assert!(parse_env_list(key).is_empty());
        // whitespace + commas
        unsafe { std::env::set_var(key, " memory_* , , file-* ") };
        assert_eq!(
            parse_env_list(key),
            vec!["memory_*".to_string(), "file-*".to_string()]
        );
        unsafe { std::env::remove_var(key) };
    }
}
