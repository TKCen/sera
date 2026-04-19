//! Runtime-side skill dispatch — wires [`sera_skills::TriggerDispatcher`]
//! to a [`SkillRegistry`] for per-turn activation.
//!
//! This is the basic implementation: given a turn's user-message content,
//! we compute which skills should fire (via keyword / `SkillTrigger` match)
//! and activate them on the shared registry. Callers typically invoke
//! [`SkillDispatchEngine::on_turn`] from the harness before the think step
//! so activated skills contribute their `context_injection` to the prompt.

use std::path::Path;
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
}

impl SkillDispatchEngine {
    /// Construct an empty engine.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                dispatcher: TriggerDispatcher::new(),
                registry: SkillRegistry::new(),
            }),
        }
    }

    /// Register a skill. The config drives trigger matching and registry
    /// activation; the optional definition supplies keyword triggers from
    /// markdown frontmatter.
    pub fn register(&self, config: SkillConfig, definition: Option<SkillDefinition>) {
        let mut g = self.inner.lock().expect("skill engine mutex poisoned");
        g.registry.register(config.clone());
        g.dispatcher.register(config, definition);
    }

    /// Load every `*.md` skill file under `dir` (non-recursive) and register
    /// it. Parse failures are logged and skipped so one bad file does not
    /// wedge the runtime.
    pub async fn load_dir(&self, dir: &Path) -> Result<usize, SkillsError> {
        if !dir.exists() {
            return Ok(0);
        }
        let mut count = 0usize;
        let mut reader = tokio::fs::read_dir(dir).await?;
        while let Some(entry) = reader.next_entry().await? {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "md") {
                continue;
            }
            match parse_skill_markdown_file(&path).await {
                Ok(parsed) => {
                    self.register(parsed.config, Some(parsed.definition));
                    count += 1;
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "skill_dispatch: failed to parse skill markdown, skipping"
                    );
                }
            }
        }
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
        let Inner { dispatcher, registry } = &mut *g;
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
}

impl Default for SkillDispatchEngine {
    fn default() -> Self {
        Self::new()
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
}
