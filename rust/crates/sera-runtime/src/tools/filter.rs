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
    ///
    /// Pattern matching uses `pattern_matches_tool_name`, which extends
    /// `glob_match` with two legacy-compatibility shims so manifest patterns
    /// from the documented defaults still match the modern hyphenated
    /// runtime tool names: `_`↔`-` normalisation (`file_*` → `file-read`)
    /// and bare-name family aliasing (`shell` → `shell-exec`).
    pub fn matches(&self, name: &str) -> bool {
        if self
            .deny
            .iter()
            .any(|p| pattern_matches_tool_name(p, name))
        {
            return false;
        }
        if !self.allow.is_empty()
            && !self
                .allow
                .iter()
                .any(|p| pattern_matches_tool_name(p, name))
        {
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

/// Match a manifest tool pattern against a runtime tool name with the two
/// legacy-compat shims SERA's documented defaults rely on:
///
/// 1. **Direct glob match** via [`glob_match`].
/// 2. **`_` ↔ `-` normalisation** so manifest patterns written with
///    underscores (`file_*`, `session_*`) still match runtime tool names
///    written with hyphens (`file-read`, `session-spawn`) and vice versa.
/// 3. **Bare-family alias** so a literal pattern with no wildcard (`shell`)
///    also matches the hyphenated family `shell-*` (e.g. `shell-exec`).
///
/// Used for both the allow and deny sides so deny still overrides allow
/// after aliasing.
fn pattern_matches_tool_name(pattern: &str, name: &str) -> bool {
    if glob_match(pattern, name) {
        return true;
    }
    let norm_pattern = pattern.replace('_', "-");
    let norm_name = name.replace('_', "-");
    if glob_match(&norm_pattern, &norm_name) {
        return true;
    }
    if !pattern.contains('*') {
        let family_prefix = format!("{norm_pattern}-");
        if norm_name.starts_with(&family_prefix) {
            return true;
        }
    }
    false
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

    // ── Legacy-compat: documented manifest patterns vs hyphenated names ──────

    /// `file_*` (manifest convention) must match the hyphenated runtime
    /// names `file-read`, `file-write`, `file-list`, `file-edit`.
    #[test]
    fn filter_underscore_glob_matches_hyphen_runtime_names() {
        let f = ToolNameFilter::from_globs(vec!["file_*".into()], vec![]);
        for name in ["file-read", "file-write", "file-list", "file-edit"] {
            assert!(f.matches(name), "expected `file_*` to allow `{name}`");
        }
        // Non-file tools must still be filtered out.
        assert!(!f.matches("shell-exec"));
        assert!(!f.matches("memory_search"));
    }

    /// Bare-name alias: `shell` must match the family `shell-exec`
    /// (and any future `shell-*` tool) without breaking exact matches.
    #[test]
    fn filter_bare_name_aliases_to_hyphen_family() {
        let f = ToolNameFilter::from_globs(vec!["shell".into()], vec![]);
        assert!(f.matches("shell-exec"));
        assert!(f.matches("shell")); // exact match still works
        assert!(!f.matches("file-read"));
    }

    /// `session_*` must match both the existing underscore tool names
    /// (`session_spawn`, …) and any hyphenated variant if/when they are
    /// renamed (`session-spawn`, …) — the test pins both directions so
    /// future renames don't silently drop schema disclosure.
    #[test]
    fn filter_session_glob_matches_both_separator_styles() {
        let f = ToolNameFilter::from_globs(vec!["session_*".into()], vec![]);
        for name in [
            "session_spawn",
            "session_yield",
            "session_send",
            "session-spawn",
            "session-yield",
            "session-send",
        ] {
            assert!(f.matches(name), "expected `session_*` to allow `{name}`");
        }
    }

    /// Deny still overrides allow after the legacy-compat aliasing — the
    /// hyphen-normalised deny pattern blocks the underscore-normalised
    /// runtime name (and vice versa), and a bare deny entry blocks the
    /// whole hyphenated family.
    #[test]
    fn filter_deny_overrides_allow_after_normalisation() {
        // `file_*` allow + `file_write` deny → underscore deny still
        // hides the hyphenated `file-write` runtime name.
        let f = ToolNameFilter::from_globs(vec!["file_*".into()], vec!["file_write".into()]);
        assert!(f.matches("file-read"));
        assert!(!f.matches("file-write"));

        // `*` allow + bare `shell` deny → blocks the entire `shell-*`
        // family via the bare-name alias rule.
        let f = ToolNameFilter::from_globs(vec!["*".into()], vec!["shell".into()]);
        assert!(f.matches("file-read"));
        assert!(!f.matches("shell-exec"));
        assert!(!f.matches("shell"));
    }

    /// The default documented manifest set
    /// (`memory_*`, `file_*`, `shell`, `session_*`) must light up every
    /// runtime tool family it was written for, both the underscore-native
    /// names (`memory_search`, `session_spawn`) and the hyphenated ones
    /// (`file-read`, `shell-exec`).
    #[test]
    fn filter_default_manifest_patterns_match_runtime_names() {
        let f = ToolNameFilter::from_globs(
            vec![
                "memory_*".into(),
                "file_*".into(),
                "shell".into(),
                "session_*".into(),
            ],
            vec![],
        );
        for name in [
            "memory_search",
            "file-read",
            "file-write",
            "file-list",
            "file-edit",
            "shell-exec",
            "session_spawn",
            "session_yield",
            "session_send",
        ] {
            assert!(
                f.matches(name),
                "default manifest patterns should allow `{name}`"
            );
        }
        // Tools not covered by the default set stay hidden.
        assert!(!f.matches("http-request"));
        assert!(!f.matches("knowledge-store"));
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
