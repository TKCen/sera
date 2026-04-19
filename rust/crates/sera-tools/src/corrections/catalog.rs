//! Hot-reloadable, per-tool correction catalog.
//!
//! Layout on disk (rooted at `~/.sera/tool-corrections/` by default):
//!
//! ```text
//! ~/.sera/tool-corrections/
//!   bash/
//!     active/corrections.yaml    # enforced rules
//!     proposed/<id>.yaml         # agent-submitted, awaiting approval
//!   runtime/
//!     active/corrections.yaml
//!     proposed/...
//! ```
//!
//! The catalog watches the `active/` directory of every tool for changes and
//! reloads the in-memory rule set on the next call — no restart needed.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use chrono::Utc;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use notify::RecommendedWatcher;
use regex::Regex;
use tracing::{debug, error, info, warn};

use super::types::{CorrectionFile, CorrectionRule, MatchKind, ToolCorrection};

/// Upper bound on enforced rules per tool. Keeps preflight cost predictable
/// and prevents a runaway YAML from stalling every call.
pub const MAX_ACTIVE_RULES_PER_TOOL: usize = 50;

/// Default debounce window for file-system events. Sub-second so tests don't
/// wait long, long enough that an editor's save-write-rename dance collapses
/// to a single reload.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(250);

/// One enforced rule plus its compiled matcher.
struct CompiledRule {
    rule: CorrectionRule,
    matcher: Matcher,
}

enum Matcher {
    Regex(Regex),
    Substring(String),
    Exact(String),
}

impl Matcher {
    fn matches(&self, text: &str) -> bool {
        match self {
            Self::Regex(r) => r.is_match(text),
            Self::Substring(s) => text.contains(s.as_str()),
            Self::Exact(s) => text == s.as_str(),
        }
    }
}

fn compile(rule: &CorrectionRule) -> Result<Matcher, String> {
    match rule.matches {
        MatchKind::Regex => Regex::new(&rule.pattern)
            .map(Matcher::Regex)
            .map_err(|e| format!("invalid regex '{}': {e}", rule.pattern)),
        MatchKind::Substring => Ok(Matcher::Substring(rule.pattern.clone())),
        MatchKind::Exact => Ok(Matcher::Exact(rule.pattern.clone())),
    }
}

/// Tool-scoped correction catalog with a file watcher.
///
/// Cheaply cloneable: shares the inner state via `Arc`.
#[derive(Clone)]
pub struct CorrectionCatalog {
    inner: Arc<CatalogInner>,
}

struct CatalogInner {
    root: PathBuf,
    rules: RwLock<HashMap<String, Vec<CompiledRule>>>,
    /// Keep the debouncer alive for the life of the catalog. Dropping it
    /// stops the watcher thread.
    _watcher: Mutex<Option<Debouncer<RecommendedWatcher>>>,
}

impl CorrectionCatalog {
    /// Create a catalog rooted at `root` and load every existing
    /// `<tool>/active/corrections.yaml` once. Returns a catalog with no
    /// watcher attached — call [`Self::watch`] to enable hot reload.
    pub fn load(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let mut rules = HashMap::new();
        if root.exists() {
            for entry in std::fs::read_dir(&root)? {
                let entry = entry?;
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let tool = match path.file_name().and_then(|s| s.to_str()) {
                    Some(s) => s.to_string(),
                    None => continue,
                };
                let active = path.join("active").join("corrections.yaml");
                if active.exists() {
                    match load_file(&active) {
                        Ok(compiled) => {
                            debug!(tool = %tool, count = compiled.len(), "loaded correction rules");
                            rules.insert(tool, compiled);
                        }
                        Err(e) => warn!(tool = %tool, error = %e, "failed to load correction YAML; skipping"),
                    }
                }
            }
        }
        Ok(Self {
            inner: Arc::new(CatalogInner {
                root,
                rules: RwLock::new(rules),
                _watcher: Mutex::new(None),
            }),
        })
    }

    /// Load and attach a file watcher. When anything under `<root>/*/active/`
    /// changes the catalog reloads the affected tool's rules.
    pub fn load_and_watch(root: impl Into<PathBuf>) -> io::Result<Self> {
        let catalog = Self::load(root)?;
        catalog.watch()?;
        Ok(catalog)
    }

    /// Enable the file watcher on an already-loaded catalog. No-op if already
    /// watching.
    pub fn watch(&self) -> io::Result<()> {
        let mut guard = self.inner._watcher.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }
        let catalog = self.clone();
        let mut debouncer = new_debouncer(WATCH_DEBOUNCE, move |res: DebounceEventResult| {
            match res {
                Ok(events) => {
                    for ev in events {
                        if let Some(tool) = tool_from_event_path(&catalog.inner.root, &ev.path) {
                            catalog.reload_tool(&tool);
                        }
                    }
                }
                Err(e) => error!(error = ?e, "correction watcher error"),
            }
        })
        .map_err(|e| io::Error::other(format!("watcher: {e}")))?;

        debouncer
            .watcher()
            .watch(&self.inner.root, notify::RecursiveMode::Recursive)
            .map_err(|e| io::Error::other(format!("watch root: {e}")))?;

        *guard = Some(debouncer);
        info!(root = %self.inner.root.display(), "correction catalog watcher started");
        Ok(())
    }

    /// Re-read `<tool>/active/corrections.yaml` into memory. Called by the
    /// watcher; also available for tests that need a deterministic reload.
    pub fn reload_tool(&self, tool: &str) {
        let path = self.inner.root.join(tool).join("active").join("corrections.yaml");
        let mut guard = self.inner.rules.write().unwrap();
        if path.exists() {
            match load_file(&path) {
                Ok(compiled) => {
                    debug!(tool, count = compiled.len(), "reloaded correction rules");
                    guard.insert(tool.to_string(), compiled);
                }
                Err(e) => warn!(tool, error = %e, "failed to reload correction YAML"),
            }
        } else {
            guard.remove(tool);
        }
    }

    /// Check an invocation against the tool's active rules. Returns the
    /// first matching blocked or warning correction. Updates hit metadata
    /// in memory on match.
    pub fn check(&self, tool: &str, invocation_text: &str) -> Option<ToolCorrection> {
        let mut guard = self.inner.rules.write().unwrap();
        let rules = guard.get_mut(tool)?;
        for entry in rules.iter_mut() {
            if entry.matcher.matches(invocation_text) {
                entry.rule.hit_count = entry.rule.hit_count.saturating_add(1);
                entry.rule.last_hit = Some(Utc::now());
                debug!(
                    tool,
                    rule_id = %entry.rule.id,
                    hit_count = entry.rule.hit_count,
                    "correction rule fired",
                );
                return Some(entry.rule.to_correction());
            }
        }
        None
    }

    /// Return the current rule IDs enforced for `tool` (for diagnostics).
    pub fn rule_ids(&self, tool: &str) -> Vec<String> {
        self.inner
            .rules
            .read()
            .unwrap()
            .get(tool)
            .map(|rs| rs.iter().map(|r| r.rule.id.clone()).collect())
            .unwrap_or_default()
    }

    /// Number of active rules currently enforced for `tool`.
    pub fn len(&self, tool: &str) -> usize {
        self.inner
            .rules
            .read()
            .unwrap()
            .get(tool)
            .map(|rs| rs.len())
            .unwrap_or(0)
    }

    /// Path root the catalog watches. Useful for admin tooling.
    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// Write a proposed rule to `<tool>/proposed/<id>.yaml`. Does not modify
    /// the enforced (`active`) set. The admin path promotes proposed → active
    /// by moving the file into `active/corrections.yaml`.
    pub fn propose(&self, tool: &str, rule: CorrectionRule) -> io::Result<PathBuf> {
        validate_tool_name(tool)?;
        let dir = self.inner.root.join(tool).join("proposed");
        std::fs::create_dir_all(&dir)?;
        let safe_id = sanitize_id(&rule.id);
        let path = dir.join(format!("{safe_id}.yaml"));
        let file = CorrectionFile { rules: vec![rule] };
        let yaml = serde_yaml::to_string(&file)
            .map_err(|e| io::Error::other(format!("serialize proposed rule: {e}")))?;
        std::fs::write(&path, yaml)?;
        info!(tool, path = %path.display(), "wrote proposed correction rule");
        Ok(path)
    }

    /// Move a proposed rule into the active set. Used by the admin CLI /
    /// approval endpoint; the meta-tool does not call this directly.
    pub fn approve(&self, tool: &str, rule_id: &str) -> io::Result<()> {
        validate_tool_name(tool)?;
        let safe_id = sanitize_id(rule_id);
        let src = self.inner.root.join(tool).join("proposed").join(format!("{safe_id}.yaml"));
        if !src.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no proposed rule '{rule_id}' for tool '{tool}'"),
            ));
        }
        let proposed_file: CorrectionFile = serde_yaml::from_str(&std::fs::read_to_string(&src)?)
            .map_err(|e| io::Error::other(format!("parse proposed: {e}")))?;

        let active_dir = self.inner.root.join(tool).join("active");
        std::fs::create_dir_all(&active_dir)?;
        let active_path = active_dir.join("corrections.yaml");
        let mut current: CorrectionFile = if active_path.exists() {
            serde_yaml::from_str(&std::fs::read_to_string(&active_path)?)
                .map_err(|e| io::Error::other(format!("parse active: {e}")))?
        } else {
            CorrectionFile::default()
        };
        for rule in proposed_file.rules {
            if current.rules.iter().any(|r| r.id == rule.id) {
                continue;
            }
            current.rules.push(rule);
        }
        if current.rules.len() > MAX_ACTIVE_RULES_PER_TOOL {
            return Err(io::Error::other(format!(
                "tool '{tool}' would exceed cap of {MAX_ACTIVE_RULES_PER_TOOL} active rules"
            )));
        }
        let yaml = serde_yaml::to_string(&current)
            .map_err(|e| io::Error::other(format!("serialize active: {e}")))?;
        std::fs::write(&active_path, yaml)?;
        std::fs::remove_file(&src)?;
        self.reload_tool(tool);
        info!(tool, rule_id, "approved correction rule");
        Ok(())
    }
}

fn load_file(path: &Path) -> Result<Vec<CompiledRule>, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let file: CorrectionFile =
        serde_yaml::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))?;
    let mut out = Vec::with_capacity(file.rules.len());
    for rule in file.rules.into_iter().take(MAX_ACTIVE_RULES_PER_TOOL) {
        match compile(&rule) {
            Ok(matcher) => out.push(CompiledRule { rule, matcher }),
            Err(e) => warn!(rule_id = %rule.id, error = %e, "skipping invalid correction rule"),
        }
    }
    Ok(out)
}

fn tool_from_event_path(root: &Path, event_path: &Path) -> Option<String> {
    let rel = event_path.strip_prefix(root).ok()?;
    let first = rel.components().next()?;
    let name = first.as_os_str().to_str()?;
    Some(name.to_string())
}

fn validate_tool_name(tool: &str) -> io::Result<()> {
    if tool.is_empty()
        || tool.contains('/')
        || tool.contains('\\')
        || tool.contains("..")
        || tool.starts_with('.')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid tool name: '{tool}'"),
        ));
    }
    Ok(())
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corrections::types::{CorrectionFile, CorrectionRule, CorrectionSeverity, MatchKind};
    use tempfile::TempDir;

    fn write_active(root: &Path, tool: &str, rules: Vec<CorrectionRule>) {
        let dir = root.join(tool).join("active");
        std::fs::create_dir_all(&dir).unwrap();
        let file = CorrectionFile { rules };
        std::fs::write(dir.join("corrections.yaml"), serde_yaml::to_string(&file).unwrap())
            .unwrap();
    }

    #[test]
    fn regex_rule_matches_and_blocks() {
        let dir = TempDir::new().unwrap();
        write_active(
            dir.path(),
            "bash",
            vec![CorrectionRule::new(
                "sleep-chain",
                r"sleep\s+\d+\s*&&",
                "Use until-loop",
                "seed",
            )],
        );
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        let got = cat.check("bash", "sleep 30 && gh pr checks 950").unwrap();
        assert!(got.is_blocked());
        assert!(got.render().contains("until-loop"));
    }

    #[test]
    fn substring_rule_matches() {
        let dir = TempDir::new().unwrap();
        let mut rule = CorrectionRule::new("pipe-grep", "| grep", "use read_file", "seed");
        rule.matches = MatchKind::Substring;
        write_active(dir.path(), "bash", vec![rule]);
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        assert!(cat.check("bash", "cat foo.txt | grep bar").is_some());
        assert!(cat.check("bash", "ls -la").is_none());
    }

    #[test]
    fn warn_severity_emits_warning() {
        let dir = TempDir::new().unwrap();
        let mut rule = CorrectionRule::new("plain-http", "curl http://", "prefer https://", "seed");
        rule.matches = MatchKind::Substring;
        rule.severity = CorrectionSeverity::Warn;
        write_active(dir.path(), "bash", vec![rule]);
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        let got = cat.check("bash", "curl http://example.com").unwrap();
        assert!(!got.is_blocked());
        assert!(got.render().contains("Warning"));
    }

    #[test]
    fn no_match_returns_none() {
        let dir = TempDir::new().unwrap();
        write_active(
            dir.path(),
            "bash",
            vec![CorrectionRule::new("r1", r"^rm -rf /$", "dangerous", "seed")],
        );
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        assert!(cat.check("bash", "ls").is_none());
    }

    #[test]
    fn per_tool_scoping_isolates_rules() {
        // A rule registered under `bash` must not fire for `runtime` calls.
        let dir = TempDir::new().unwrap();
        write_active(
            dir.path(),
            "bash",
            vec![CorrectionRule::new("broad", "foo", "no foo", "seed")],
        );
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        assert!(cat.check("bash", "foo bar").is_some());
        assert!(cat.check("runtime", "foo bar").is_none());
    }

    #[test]
    fn invalid_regex_is_skipped() {
        let dir = TempDir::new().unwrap();
        let bad = CorrectionRule::new("bad", "[invalid", "correction", "seed");
        let good = CorrectionRule::new("good", r"echo\s+hi", "correction", "seed");
        write_active(dir.path(), "bash", vec![bad, good]);
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        assert_eq!(cat.len("bash"), 1);
        assert_eq!(cat.rule_ids("bash"), vec!["good".to_string()]);
    }

    #[test]
    fn propose_writes_to_proposed_dir() {
        let dir = TempDir::new().unwrap();
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        let rule = CorrectionRule::new("test-rule", "foo", "use bar", "agent");
        let path = cat.propose("bash", rule).unwrap();
        assert!(path.exists());
        assert!(path.starts_with(dir.path().join("bash").join("proposed")));
        // Proposed rules are NOT enforced.
        assert!(cat.check("bash", "foo").is_none());
    }

    #[test]
    fn approve_moves_proposed_to_active() {
        let dir = TempDir::new().unwrap();
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        let rule = CorrectionRule::new("sleep-chain", r"sleep\s+\d+\s*&&", "use until", "agent");
        cat.propose("bash", rule).unwrap();
        cat.approve("bash", "sleep-chain").unwrap();
        assert_eq!(cat.len("bash"), 1);
        assert!(cat.check("bash", "sleep 10 && echo ok").is_some());
    }

    #[test]
    fn hit_count_increments_on_match() {
        let dir = TempDir::new().unwrap();
        write_active(
            dir.path(),
            "bash",
            vec![CorrectionRule::new("r1", r"echo", "no echo", "seed")],
        );
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        cat.check("bash", "echo a").unwrap();
        cat.check("bash", "echo b").unwrap();
        let guard = cat.inner.rules.read().unwrap();
        let rule = &guard.get("bash").unwrap()[0].rule;
        assert_eq!(rule.hit_count, 2);
        assert!(rule.last_hit.is_some());
    }

    #[test]
    fn cap_truncates_excess_rules() {
        let dir = TempDir::new().unwrap();
        let rules: Vec<_> = (0..MAX_ACTIVE_RULES_PER_TOOL + 10)
            .map(|i| CorrectionRule::new(format!("r{i}"), format!("pat{i}"), "fix", "seed"))
            .collect();
        write_active(dir.path(), "bash", rules);
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        assert_eq!(cat.len("bash"), MAX_ACTIVE_RULES_PER_TOOL);
    }

    #[test]
    fn reload_picks_up_new_rule() {
        let dir = TempDir::new().unwrap();
        write_active(
            dir.path(),
            "bash",
            vec![CorrectionRule::new("r1", "alpha", "fix", "seed")],
        );
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        assert!(cat.check("bash", "alpha").is_some());
        assert!(cat.check("bash", "beta").is_none());

        // Rewrite YAML, then reload.
        write_active(
            dir.path(),
            "bash",
            vec![CorrectionRule::new("r2", "beta", "fix2", "seed")],
        );
        cat.reload_tool("bash");
        assert!(cat.check("bash", "alpha").is_none());
        assert!(cat.check("bash", "beta").is_some());
    }

    #[test]
    fn reject_invalid_tool_name() {
        let dir = TempDir::new().unwrap();
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        let rule = CorrectionRule::new("r", "p", "c", "a");
        assert!(cat.propose("../escape", rule.clone()).is_err());
        assert!(cat.propose("bad/name", rule.clone()).is_err());
        assert!(cat.propose("", rule).is_err());
    }

    #[test]
    fn tool_from_event_path_extracts_first_segment() {
        let root = Path::new("/tmp/sera/tool-corrections");
        let ev = Path::new("/tmp/sera/tool-corrections/bash/active/corrections.yaml");
        assert_eq!(tool_from_event_path(root, ev).as_deref(), Some("bash"));
    }
}
