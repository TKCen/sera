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

use sera_types::config_manifest::AgentFeatureSetSpec;

use crate::types::ToolDefinition;

const ENV_ALLOW: &str = "SERA_AGENT_TOOLS_ALLOW";
const ENV_DENY: &str = "SERA_AGENT_TOOLS_DENY";
const ENV_FEATURES: &str = "SERA_AGENT_FEATURES";

const TOOLSET_MEMORY: &str = "memory";
const TOOLSET_CONTEXT_ENGINE: &str = "context_engine";
const TOOLSET_SKILLS: &str = "skills";
const TOOLSET_SESSION_SEARCH: &str = "session_search";

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
        if self.deny.iter().any(|p| pattern_matches_tool_name(p, name)) {
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

/// Parse `SERA_AGENT_FEATURES` as an [`AgentFeatureSetSpec`].
///
/// Missing, empty, or invalid JSON falls back to the all-default feature set,
/// which [`FeatureSurfaceToolFilter`] treats as a legacy pass-through. The
/// gateway always sets this env var for stdio children so per-agent explicit
/// feature gates cannot leak from a previous process environment.
pub fn agent_features_from_env() -> AgentFeatureSetSpec {
    std::env::var(ENV_FEATURES)
        .ok()
        .and_then(|raw| {
            let raw = raw.trim();
            if raw.is_empty() {
                None
            } else {
                serde_json::from_str::<AgentFeatureSetSpec>(raw).ok()
            }
        })
        .unwrap_or_default()
}

/// Match a manifest tool pattern against a runtime tool name with the two
/// legacy-compat shims SERA's documented defaults rely on:
///
/// 1. **Direct glob match** via [`glob_match`].
/// 2. **`_` ↔ `-` normalisation** so manifest patterns written with
///    underscores (`file_*`, `delegation_*`) still match runtime tool names
///    written with hyphens (`file-read`, `delegation-spawn`) and vice versa.
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

/// Hermes-parity surface gate for model-visible tool schemas.
///
/// `ToolNameFilter` handles legacy allow/deny globs. This filter handles the
/// newer per-agent `features.toolsets` surfaces: memory, external memory
/// providers, context-engine tools, `session_search`, and skills. A default
/// `AgentFeatureSetSpec` is a deliberate legacy pass-through so existing agents
/// with no `features:` block keep the same visible tools until they opt in to
/// explicit surface gating.
#[derive(Debug, Clone)]
pub struct FeatureSurfaceToolFilter<'a> {
    features: &'a AgentFeatureSetSpec,
}

impl<'a> FeatureSurfaceToolFilter<'a> {
    pub fn new(features: &'a AgentFeatureSetSpec) -> Self {
        Self { features }
    }

    pub fn is_pass_through(&self) -> bool {
        self.features.is_default()
    }

    pub fn matches(&self, name: &str) -> bool {
        if self.is_pass_through() {
            return true;
        }

        if is_session_search_tool_name(name) {
            return self.toolset_enabled(TOOLSET_SESSION_SEARCH);
        }

        if is_skill_tool_name(name) {
            return self.toolset_enabled(TOOLSET_SKILLS);
        }

        if is_context_engine_tool_name(name) {
            return self.context_engine_tool_allowed(name);
        }

        if is_semantic_memory_tool_name(name) {
            return self.memory_toolset_enabled() && self.features.semantic_memory.enabled;
        }

        if is_builtin_memory_tool_name(name) {
            let Some(memory_gate) = self.features.toolsets.get(TOOLSET_MEMORY) else {
                return false;
            };
            return memory_gate.enabled
                && memory_gate.builtin_memory_tool
                && self.features.identity_memory.enabled
                && self.features.identity_memory.memory_tool;
        }

        if is_memory_provider_tool_name(name) {
            return self.memory_provider_tool_allowed();
        }

        true
    }

    pub fn filter_definitions(&self, defs: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
        if self.is_pass_through() {
            return defs;
        }
        defs.into_iter()
            .filter(|d| self.matches(&d.function.name))
            .collect()
    }

    fn toolset_enabled(&self, name: &str) -> bool {
        self.features
            .toolsets
            .get(name)
            .map(|gate| gate.enabled)
            .unwrap_or(false)
    }

    fn memory_toolset_enabled(&self) -> bool {
        self.toolset_enabled(TOOLSET_MEMORY)
    }

    fn context_engine_tool_allowed(&self, name: &str) -> bool {
        if !self.features.context_engine.enabled || !self.features.context_engine.expose_tools {
            return false;
        }
        let Some(gate) = self.features.toolsets.get(TOOLSET_CONTEXT_ENGINE) else {
            return false;
        };
        if !gate.enabled {
            return false;
        }
        matches_intersection_or_single_allow(
            name,
            &self.features.context_engine.tool_allow,
            &gate.tool_allow,
        )
    }

    fn memory_provider_tool_allowed(&self) -> bool {
        let Some(memory_gate) = self.features.toolsets.get(TOOLSET_MEMORY) else {
            return false;
        };
        if !memory_gate.enabled
            || !memory_gate.provider_tools
            || !self.features.memory_providers.enabled
        {
            return false;
        }

        if self
            .features
            .memory_providers
            .allow_multiple
            .unwrap_or(false)
        {
            return self
                .features
                .memory_providers
                .providers
                .values()
                .any(provider_exposes_tools);
        }

        let Some(active) = self.features.memory_providers.active.as_deref() else {
            return false;
        };
        self.features
            .memory_providers
            .providers
            .get(active)
            .map(provider_exposes_tools)
            .unwrap_or(false)
    }
}

fn provider_exposes_tools(
    provider: &sera_types::config_manifest::MemoryProviderFeatureSpec,
) -> bool {
    provider.enabled && provider.expose_tools.unwrap_or(false)
}

fn matches_intersection_or_single_allow(name: &str, first: &[String], second: &[String]) -> bool {
    let first_empty = first.is_empty();
    let second_empty = second.is_empty();
    match (first_empty, second_empty) {
        (true, true) => true,
        (false, true) => matches_any_allow(first, name),
        (true, false) => matches_any_allow(second, name),
        (false, false) => matches_any_allow(first, name) && matches_any_allow(second, name),
    }
}

fn matches_any_allow(patterns: &[String], name: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| pattern_matches_tool_name(pattern, name))
}

fn is_builtin_memory_tool_name(name: &str) -> bool {
    matches!(
        name,
        "memory" | "memory_read" | "memory-write" | "memory_write" | "memory_synthesize"
    )
}

fn is_semantic_memory_tool_name(name: &str) -> bool {
    matches!(name, "memory_search" | "memory-search")
}

fn is_memory_provider_tool_name(name: &str) -> bool {
    name.starts_with("hindsight_")
        || name.starts_with("hindsight-")
        || name.starts_with("memory_provider_")
        || name.starts_with("memory-provider-")
}

fn is_context_engine_tool_name(name: &str) -> bool {
    name.starts_with("lcm_")
        || name.starts_with("lcm-")
        || name.starts_with("context_engine_")
        || name.starts_with("context-engine-")
}

fn is_session_search_tool_name(name: &str) -> bool {
    matches!(name, "session_search" | "session-search")
}

fn is_skill_tool_name(name: &str) -> bool {
    matches!(
        name,
        "skill-search" | "skill_search" | "skills" | "skills_list"
    ) || name.starts_with("skill_")
        || name.starts_with("skill-")
        || name.starts_with("skills_")
        || name.starts_with("skills-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionDefinition, ToolDefinition};
    use sera_types::config_manifest::{
        AgentFeatureSetSpec, ContextEngineFeatureSpec, IdentityMemoryFeatureSpec,
        MemoryProviderFeatureSpec, MemoryProvidersFeatureSpec, SemanticMemoryFeatureSpec,
        ToolsetGateSpec,
    };
    use std::collections::HashMap;

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

    /// `delegation_*` must match both the underscore tool names
    /// (`delegation_spawn`, …) and hyphenated variants (`delegation-spawn`, …)
    /// so future separator-style changes don't silently drop schema disclosure.
    #[test]
    fn filter_delegation_glob_matches_both_separator_styles() {
        let f = ToolNameFilter::from_globs(vec!["delegation_*".into()], vec![]);
        for name in [
            "delegation_spawn",
            "delegation_list",
            "delegation_yield",
            "delegation_send",
            "delegation-spawn",
            "delegation-list",
            "delegation-yield",
            "delegation-send",
        ] {
            assert!(f.matches(name), "expected `delegation_*` to allow `{name}`");
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
    /// (`memory_*`, `file_*`, `shell`, `delegation_*`) must light up every
    /// runtime tool family it was written for, both the underscore-native
    /// names (`memory_search`, `delegation_spawn`) and the hyphenated ones
    /// (`file-read`, `shell-exec`).
    #[test]
    fn filter_default_manifest_patterns_match_runtime_names() {
        let f = ToolNameFilter::from_globs(
            vec![
                "memory_*".into(),
                "file_*".into(),
                "shell".into(),
                "delegation_*".into(),
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
            "delegation_spawn",
            "delegation_list",
            "delegation_yield",
            "delegation_send",
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
    fn feature_surface_filter_default_features_are_legacy_pass_through() {
        let features = AgentFeatureSetSpec::default();
        let f = FeatureSurfaceToolFilter::new(&features);
        let defs = vec![
            def("memory_write"),
            def("memory_search"),
            def("hindsight_recall"),
            def("lcm_grep"),
            def("session_search"),
            def("skill-search"),
            def("file-read"),
        ];
        let names: Vec<_> = f
            .filter_definitions(defs)
            .into_iter()
            .map(|d| d.function.name)
            .collect();
        assert_eq!(
            names,
            vec![
                "memory_write",
                "memory_search",
                "hindsight_recall",
                "lcm_grep",
                "session_search",
                "skill-search",
                "file-read"
            ]
        );
    }

    #[test]
    fn feature_surface_filter_hides_disabled_surfaces_but_preserves_unclassified_tools() {
        let features = AgentFeatureSetSpec {
            toolsets: HashMap::from([(
                TOOLSET_MEMORY.to_string(),
                ToolsetGateSpec {
                    enabled: false,
                    builtin_memory_tool: true,
                    provider_tools: true,
                    tool_allow: vec![],
                },
            )]),
            ..AgentFeatureSetSpec::default()
        };
        let f = FeatureSurfaceToolFilter::new(&features);
        let defs = vec![
            def("memory_write"),
            def("memory_search"),
            def("hindsight_recall"),
            def("lcm_grep"),
            def("skill-search"),
            def("file-read"),
        ];
        let names: Vec<_> = f
            .filter_definitions(defs)
            .into_iter()
            .map(|d| d.function.name)
            .collect();
        assert_eq!(names, vec!["file-read"]);
    }

    #[test]
    fn feature_surface_filter_enforces_each_explicit_surface_gate() {
        let mut providers = HashMap::new();
        providers.insert(
            "hindsight".to_string(),
            MemoryProviderFeatureSpec {
                enabled: true,
                recall: true,
                reflect: true,
                retain_policy: Some("disabled".to_string()),
                expose_tools: Some(true),
                tags: vec![],
            },
        );
        let features = AgentFeatureSetSpec {
            identity_memory: IdentityMemoryFeatureSpec {
                enabled: true,
                memory_tool: true,
                ..IdentityMemoryFeatureSpec::default()
            },
            semantic_memory: SemanticMemoryFeatureSpec {
                enabled: true,
                ..SemanticMemoryFeatureSpec::default()
            },
            memory_providers: MemoryProvidersFeatureSpec {
                enabled: true,
                active: Some("hindsight".to_string()),
                providers,
                ..MemoryProvidersFeatureSpec::default()
            },
            context_engine: ContextEngineFeatureSpec {
                enabled: true,
                expose_tools: true,
                tool_allow: vec!["lcm_*".to_string(), "lcm_expand".to_string()],
                ..ContextEngineFeatureSpec::default()
            },
            toolsets: HashMap::from([
                (
                    TOOLSET_MEMORY.to_string(),
                    ToolsetGateSpec {
                        enabled: true,
                        builtin_memory_tool: true,
                        provider_tools: true,
                        tool_allow: vec![],
                    },
                ),
                (
                    TOOLSET_CONTEXT_ENGINE.to_string(),
                    ToolsetGateSpec {
                        enabled: true,
                        builtin_memory_tool: false,
                        provider_tools: false,
                        tool_allow: vec!["lcm_grep".to_string(), "lcm_expand".to_string()],
                    },
                ),
                (
                    TOOLSET_SESSION_SEARCH.to_string(),
                    ToolsetGateSpec {
                        enabled: true,
                        ..ToolsetGateSpec::default()
                    },
                ),
                (
                    TOOLSET_SKILLS.to_string(),
                    ToolsetGateSpec {
                        enabled: true,
                        ..ToolsetGateSpec::default()
                    },
                ),
            ]),
        };
        let f = FeatureSurfaceToolFilter::new(&features);
        let defs = vec![
            def("memory_write"),
            def("memory_search"),
            def("hindsight_recall"),
            def("lcm_grep"),
            def("lcm_expand"),
            def("lcm_describe"),
            def("session_search"),
            def("skill-search"),
            def("file-read"),
        ];
        let names: Vec<_> = f
            .filter_definitions(defs)
            .into_iter()
            .map(|d| d.function.name)
            .collect();
        assert_eq!(
            names,
            vec![
                "memory_write",
                "memory_search",
                "hindsight_recall",
                "lcm_grep",
                "lcm_expand",
                "session_search",
                "skill-search",
                "file-read"
            ],
            "context-engine allow lists are intersected, so lcm_describe is hidden"
        );
    }

    #[test]
    fn agent_features_from_env_parses_json_and_invalid_defaults() {
        let key = ENV_FEATURES;
        unsafe {
            std::env::set_var(
                key,
                r#"{"toolsets":{"memory":{"enabled":true,"builtin_memory_tool":true}}}"#,
            )
        };
        let parsed = agent_features_from_env();
        assert!(parsed.toolsets[TOOLSET_MEMORY].enabled);
        assert!(parsed.toolsets[TOOLSET_MEMORY].builtin_memory_tool);

        unsafe { std::env::set_var(key, "not-json") };
        assert!(agent_features_from_env().is_default());
        unsafe { std::env::remove_var(key) };
        assert!(agent_features_from_env().is_default());
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
