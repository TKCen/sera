//! Runtime-side skill dispatch — wires [`sera_skills::TriggerDispatcher`]
//! to a [`SkillRegistry`] for per-turn activation.
//!
//! This is the basic implementation: given a turn's user-message content,
//! we compute which skills should fire (via keyword / `SkillTrigger` match)
//! and activate them on the shared registry. Callers typically invoke
//! [`SkillDispatchEngine::on_turn`] from the harness before the think step
//! so activated skills contribute their `context_injection` to the prompt.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sera_skills::{SkillsError, TriggerDispatcher, parse_skill_markdown_file};
use sera_types::skill::{SkillConfig, SkillDefinition, SkillRegistry};

pub use sera_skills::{MatchReason, SkillMatch};

/// Thread-safe container pairing a [`TriggerDispatcher`] with a
/// [`SkillRegistry`]. Both are held behind a single mutex so dispatch +
/// activation are atomic relative to concurrent turns.
pub struct SkillDispatchEngine {
    inner: Mutex<Inner>,
}

struct Inner {
    dispatcher: TriggerDispatcher,
    registry: SkillRegistry,
    source_dirs: Vec<PathBuf>,
    manual_entries: Vec<(SkillConfig, Option<SkillDefinition>)>,
}

impl Inner {
    fn empty() -> Self {
        Self {
            dispatcher: TriggerDispatcher::new(),
            registry: SkillRegistry::new(),
            source_dirs: Vec::new(),
            manual_entries: Vec::new(),
        }
    }

    fn register_loaded(&mut self, config: SkillConfig, definition: Option<SkillDefinition>) {
        self.registry.register(config.clone());
        self.dispatcher.register(config, definition);
    }

    fn register_manual(&mut self, config: SkillConfig, definition: Option<SkillDefinition>) {
        self.manual_entries.retain(|(existing, _)| existing.name != config.name);
        self.manual_entries.push((config.clone(), definition.clone()));
        self.register_loaded(config, definition);
    }
}

impl SkillDispatchEngine {
    /// Construct an empty engine.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::empty()),
        }
    }

    /// Register a skill. The config drives trigger matching and registry
    /// activation; the optional definition supplies keyword triggers from
    /// markdown frontmatter.
    pub fn register(&self, config: SkillConfig, definition: Option<SkillDefinition>) {
        let mut g = self.inner.lock().expect("skill engine mutex poisoned");
        g.register_manual(config, definition);
    }

    /// Load every `*.md` skill file under `dir` (non-recursive) and register
    /// it. Parse failures are logged and skipped so one bad file does not
    /// wedge the runtime.
    pub async fn load_dir(&self, dir: &Path) -> Result<usize, SkillsError> {
        let loaded = collect_skill_dir(dir).await?;
        let count = loaded.len();
        let mut g = self.inner.lock().expect("skill engine mutex poisoned");
        let dir = dir.to_path_buf();
        if !g.source_dirs.iter().any(|existing| existing == &dir) {
            g.source_dirs.push(dir);
        }
        for (config, definition) in loaded {
            g.register_loaded(config, Some(definition));
        }
        Ok(count)
    }

    /// Reload every previously loaded skill directory from disk and replace
    /// the in-memory dispatcher/registry. Active skill names that still exist
    /// after reload remain active so updated context injections take effect
    /// without a process restart.
    pub async fn reload_registered_dirs(&self) -> Result<usize, SkillsError> {
        let (source_dirs, manual_entries, active_names) = {
            let g = self.inner.lock().expect("skill engine mutex poisoned");
            (
                g.source_dirs.clone(),
                g.manual_entries.clone(),
                g.registry
                    .active_skill_names()
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
            )
        };

        if source_dirs.is_empty() {
            return Ok(self.registered_count());
        }

        let mut loaded = Vec::new();
        for dir in &source_dirs {
            loaded.extend(collect_skill_dir(dir).await?);
        }
        let count = loaded.len();

        let mut replacement = Inner::empty();
        replacement.source_dirs = source_dirs;
        replacement.manual_entries = manual_entries.clone();
        for (config, definition) in loaded {
            replacement.register_loaded(config, Some(definition));
        }
        // Preserve programmatic registrations across disk reloads. Register
        // them after disk-loaded skills so embedders can intentionally
        // override a scanned skill by name.
        for (config, definition) in manual_entries {
            replacement.register_loaded(config, definition);
        }
        let mut g = self.inner.lock().expect("skill engine mutex poisoned");
        restore_active_names(&mut replacement, active_names, &g.registry);
        *g = replacement;
        Ok(count)
    }

    /// Inspect the matches for a given turn content without activating.
    pub fn matches(&self, content: &str) -> Vec<SkillMatch> {
        self.inner
            .lock()
            .expect("skill engine mutex poisoned")
            .dispatcher
            .dispatch(content)
    }

    /// Activate all skills whose triggers match the content. Returns the set
    /// of newly activated skills (skills already active are not re-fired).
    pub fn on_turn(&self, content: &str) -> Vec<SkillMatch> {
        let mut g = self.inner.lock().expect("skill engine mutex poisoned");
        let Inner {
            dispatcher,
            registry,
            ..
        } = &mut *g;
        dispatcher.fire(content, registry)
    }

    /// Returns the `context_injection` strings for every currently-active
    /// skill. Intended to be appended to the system prompt on every turn.
    pub fn active_context_injections(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("skill engine mutex poisoned")
            .registry
            .context_injections()
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// Deactivate a skill by name. Silently ignores unknown / inactive names.
    pub fn deactivate(&self, name: &str) {
        let mut g = self.inner.lock().expect("skill engine mutex poisoned");
        let _ = g.registry.deactivate(name);
    }

    /// Number of registered skills.
    pub fn registered_count(&self) -> usize {
        self.inner
            .lock()
            .expect("skill engine mutex poisoned")
            .dispatcher
            .len()
    }

    /// Registered skill names in deterministic order for truthful capability
    /// self-introspection. This exposes names only; callers should not leak
    /// arbitrary skill bodies or config payloads into user-visible text.
    pub fn registered_skill_names(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("skill engine mutex poisoned")
            .registry
            .skill_configs()
            .into_iter()
            .map(|cfg| cfg.name.clone())
            .collect()
    }

    /// Prepare skill context from the currently loaded in-memory registry
    /// without reloading from disk. This is mainly a fallback/test helper;
    /// long-running turn loops should call [`Self::prepare_turn_context`] so
    /// `skill-manage patch` effects are visible without process restart.
    pub fn prepare_loaded_turn_context(&self, turn_content: &str) -> (Vec<SkillMatch>, Vec<String>) {
        let fired = self.on_turn(turn_content);
        let injections = self.active_context_injections();
        (fired, injections)
    }

    /// Refresh registered skill directories from disk, then prepare skill
    /// context for a turn. This is the integration seam for long-running
    /// harnesses: call before the think step and prepend/append returned
    /// injections to the context window. Returns `(newly_fired, injections)`.
    pub async fn prepare_turn_context(
        &self,
        turn_content: &str,
    ) -> Result<(Vec<SkillMatch>, Vec<String>), SkillsError> {
        self.reload_registered_dirs().await?;
        Ok(self.prepare_loaded_turn_context(turn_content))
    }

    /// Active skill names in deterministic order for self-introspection.
    pub fn active_skill_names(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("skill engine mutex poisoned")
            .registry
            .active_skill_names()
            .into_iter()
            .map(String::from)
            .collect()
    }
}

async fn collect_skill_dir(
    dir: &Path,
) -> Result<Vec<(SkillConfig, SkillDefinition)>, SkillsError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut loaded = Vec::new();
    let mut reader = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = reader.next_entry().await? {
        let path = entry.path();
        // Single-file: top-level *.md files
        if path.is_file() && path.extension().is_some_and(|e| e == "md") {
            match parse_skill_markdown_file(&path).await {
                Ok(parsed) => loaded.push((parsed.config, parsed.definition)),
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "skill_dispatch: failed to parse skill markdown, skipping"
                    );
                }
            }
        // Directory-style: <name>/SKILL.md (created by skill-manage)
        } else if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.exists() {
                match parse_skill_markdown_file(&skill_md).await {
                    Ok(parsed) => loaded.push((parsed.config, parsed.definition)),
                    Err(e) => {
                        tracing::warn!(
                            path = %skill_md.display(),
                            error = %e,
                            "skill_dispatch: failed to parse directory skill, skipping"
                        );
                    }
                }
            }
        }
    }
    Ok(loaded)
}

impl Default for SkillDispatchEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn restore_active_names(
    replacement: &mut Inner,
    mut snapshotted_active_names: Vec<String>,
    current_registry: &SkillRegistry,
) {
    snapshotted_active_names.extend(
        current_registry
            .active_skill_names()
            .into_iter()
            .map(String::from),
    );
    snapshotted_active_names.sort();
    snapshotted_active_names.dedup();

    for name in snapshotted_active_names {
        let _ = replacement.registry.activate(&name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::skill::{SkillMode, SkillTrigger};

    fn cfg(name: &str, trigger: SkillTrigger, injection: Option<&str>) -> SkillConfig {
        SkillConfig {
            name: name.into(),
            version: "1.0.0".into(),
            description: "test".into(),
            mode: SkillMode::OnDemand,
            trigger,
            tools: vec![],
            context_injection: injection.map(String::from),
            config: serde_json::json!({}),
        }
    }

    #[test]
    fn on_turn_activates_event_matched_skill() {
        let eng = SkillDispatchEngine::new();
        eng.register(
            cfg("reviewer", SkillTrigger::Event("review".into()), Some("You review code.")),
            None,
        );

        let fired = eng.on_turn("please review this diff");
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].name, "reviewer");

        let inj = eng.active_context_injections();
        assert_eq!(inj, vec!["You review code."]);
    }

    #[test]
    fn on_turn_does_not_fire_manual() {
        let eng = SkillDispatchEngine::new();
        eng.register(cfg("manual", SkillTrigger::Manual, None), None);
        assert!(eng.on_turn("any content").is_empty());
        assert!(eng.active_context_injections().is_empty());
    }

    #[test]
    fn on_turn_is_idempotent_across_turns() {
        let eng = SkillDispatchEngine::new();
        eng.register(cfg("r", SkillTrigger::Event("go".into()), Some("ctx")), None);

        // First turn fires.
        assert_eq!(eng.on_turn("let's go").len(), 1);
        // Second turn with matching content must not re-fire the already-active skill.
        assert!(eng.on_turn("go again").is_empty());
        // Context injection is still applied because the skill remains active.
        assert_eq!(eng.active_context_injections(), vec!["ctx"]);
    }

    #[test]
    fn deactivate_removes_context_injection() {
        let eng = SkillDispatchEngine::new();
        eng.register(cfg("r", SkillTrigger::Always, Some("ctx")), None);
        eng.on_turn("hi");
        assert_eq!(eng.active_context_injections().len(), 1);

        eng.deactivate("r");
        assert!(eng.active_context_injections().is_empty());
    }

    #[test]
    fn matches_does_not_activate() {
        let eng = SkillDispatchEngine::new();
        eng.register(cfg("r", SkillTrigger::Event("go".into()), Some("ctx")), None);

        let m = eng.matches("go now");
        assert_eq!(m.len(), 1);
        // `matches` is read-only — nothing activated.
        assert!(eng.active_context_injections().is_empty());
    }

    #[tokio::test]
    async fn load_dir_returns_zero_for_missing_path() {
        let eng = SkillDispatchEngine::new();
        let n = eng
            .load_dir(Path::new("/tmp/does/not/exist/skills-xyz"))
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn load_dir_reads_markdown_files() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("hello.md");
        tokio::fs::write(
            &path,
            "---\nname: hello\nversion: 1.0.0\ntriggers:\n  - hi\n---\nbody\n",
        )
        .await
        .unwrap();
        // A non-markdown file is skipped.
        tokio::fs::write(tmp.path().join("README.txt"), "ignore me")
            .await
            .unwrap();

        let eng = SkillDispatchEngine::new();
        let n = eng.load_dir(tmp.path()).await.unwrap();
        assert_eq!(n, 1);
        assert_eq!(eng.registered_count(), 1);

        let fired = eng.on_turn("hi there");
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].name, "hello");
    }

    #[tokio::test]
    async fn load_dir_reads_directory_style_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("my-skill");
        tokio::fs::create_dir_all(&skill_dir).await.unwrap();
        tokio::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: my-skill\nversion: 1.0.0\ntriggers:\n  - greet\n---\nbody\n",
        )
        .await
        .unwrap();
        // Also a single-file skill alongside it.
        tokio::fs::write(
            tmp.path().join("other.md"),
            "---\nname: other\nversion: 1.0.0\n---\nbody\n",
        )
        .await
        .unwrap();

        let eng = SkillDispatchEngine::new();
        let n = eng.load_dir(tmp.path()).await.unwrap();
        assert_eq!(n, 2);
        assert_eq!(eng.registered_count(), 2);

        let fired = eng.on_turn("greet me");
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].name, "my-skill");
    }

    #[tokio::test]
    async fn load_dir_skips_invalid_markdown() {
        let tmp = tempfile::tempdir().unwrap();
        // No frontmatter — parse fails but load_dir continues.
        tokio::fs::write(tmp.path().join("bad.md"), "no frontmatter here\n")
            .await
            .unwrap();
        tokio::fs::write(
            tmp.path().join("good.md"),
            "---\nname: good\nversion: 1.0.0\n---\nbody\n",
        )
        .await
        .unwrap();

        let eng = SkillDispatchEngine::new();
        let n = eng.load_dir(tmp.path()).await.unwrap();
        assert_eq!(n, 1, "only the good file should register");
        assert_eq!(eng.registered_count(), 1);
    }

    #[tokio::test]
    async fn prepare_turn_context_sees_patched_triggers_without_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("hello.md");
        tokio::fs::write(
            &path,
            "---\nname: hello\nversion: 1.0.0\ntriggers:\n  - hi\n---\nold body\n",
        )
        .await
        .unwrap();

        let eng = SkillDispatchEngine::new();
        assert_eq!(eng.load_dir(tmp.path()).await.unwrap(), 1);
        assert!(eng.on_turn("welcome aboard").is_empty());

        tokio::fs::write(
            &path,
            "---\nname: hello\nversion: 1.0.0\ntriggers:\n  - welcome\n---\nnew body\n",
        )
        .await
        .unwrap();

        let (fired, injections) = eng
            .prepare_turn_context("welcome aboard")
            .await
            .unwrap();
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].name, "hello");
        assert!(injections.is_empty());
    }

    #[tokio::test]
    async fn reload_preserves_active_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("helper.md");
        tokio::fs::write(
            &path,
            "---\nname: helper\nversion: 1.0.0\ntriggers:\n  - go\n---\nold body\n",
        )
        .await
        .unwrap();

        let eng = SkillDispatchEngine::new();
        eng.load_dir(tmp.path()).await.unwrap();
        assert_eq!(eng.on_turn("go now").len(), 1);
        assert_eq!(eng.active_skill_names(), vec!["helper"]);

        tokio::fs::write(
            &path,
            "---\nname: helper\nversion: 1.0.0\ntriggers:\n  - go\n---\nnew body\n",
        )
        .await
        .unwrap();

        eng.reload_registered_dirs().await.unwrap();
        assert_eq!(eng.active_skill_names(), vec!["helper"]);
    }


    #[tokio::test]
    async fn reload_preserves_manual_registrations_alongside_loaded_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("disk.md");
        tokio::fs::write(
            &path,
            "---\nname: disk\nversion: 1.0.0\ntriggers:\n  - disk\n---\nbody\n",
        )
        .await
        .unwrap();

        let eng = SkillDispatchEngine::new();
        eng.register(
            cfg("manual", SkillTrigger::Event("manual".into()), Some("manual ctx")),
            None,
        );
        eng.load_dir(tmp.path()).await.unwrap();

        eng.reload_registered_dirs().await.unwrap();

        let fired = eng.on_turn("manual please");
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].name, "manual");
        assert_eq!(eng.active_context_injections(), vec!["manual ctx"]);

        eng.deactivate("manual");
        let fired_disk = eng.on_turn("disk please");
        assert_eq!(fired_disk.len(), 1);
        assert_eq!(fired_disk[0].name, "disk");
    }


    #[test]
    fn restore_active_names_merges_snapshot_and_current_registry() {
        let mut replacement = Inner::empty();
        replacement.register_loaded(
            cfg("snapshot-active", SkillTrigger::Event("snapshot".into()), Some("snapshot ctx")),
            None,
        );
        replacement.register_loaded(
            cfg("current-active", SkillTrigger::Event("current".into()), Some("current ctx")),
            None,
        );

        let mut current = SkillRegistry::new();
        current.register(cfg(
            "current-active",
            SkillTrigger::Event("current".into()),
            Some("current ctx"),
        ));
        current.activate("current-active").unwrap();

        restore_active_names(
            &mut replacement,
            vec!["snapshot-active".to_string()],
            &current,
        );

        assert_eq!(
            replacement.registry.active_skill_names(),
            vec!["current-active", "snapshot-active"],
        );
    }

}
