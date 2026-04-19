//! Integration tests for the tool-layer reinforcement catalog.
//!
//! Covers the three scenarios called out in the spec:
//! 1. A YAML rule in `active/` blocks a matching invocation.
//! 2. The seed bootstrap writes canonical rules that fire on the canonical
//!    bad patterns from the skill.
//! 3. Writing a new rule to `active/` is picked up on the next call — no
//!    process restart required.

use std::path::Path;
use std::time::{Duration, Instant};

use sera_tools::corrections::{
    CorrectionCatalog, CorrectionFile, CorrectionRule, DefaultPreflight, ToolPreflight,
};
use sera_tools::corrections::seed::{bash_seed_rules, seed_bash_catalog};
use tempfile::TempDir;

fn write_active(root: &Path, tool: &str, rules: Vec<CorrectionRule>) {
    let dir = root.join(tool).join("active");
    std::fs::create_dir_all(&dir).unwrap();
    let file = CorrectionFile { rules };
    std::fs::write(
        dir.join("corrections.yaml"),
        serde_yaml::to_string(&file).unwrap(),
    )
    .unwrap();
}

#[test]
fn active_yaml_rule_blocks_matching_invocation() {
    let dir = TempDir::new().unwrap();
    write_active(
        dir.path(),
        "bash",
        vec![CorrectionRule::new(
            "sleep-chain",
            r"sleep\s+\d+\s*&&",
            "Use an until-loop to poll.",
            "seed",
        )],
    );
    let catalog = CorrectionCatalog::load(dir.path()).unwrap();
    let pf = DefaultPreflight::new(catalog);

    let err = pf
        .check_invocation("bash", "sleep 30 && gh pr checks 950")
        .expect_err("must be blocked");
    assert!(err.is_blocked(), "correction should be a block, got {err:?}");
    assert!(err.render().contains("until-loop"));

    // Unrelated invocations must pass through untouched.
    assert!(pf.check_invocation("bash", "ls -la").is_ok());
}

#[test]
fn seed_bootstrap_catches_every_canonical_antipattern() {
    let dir = TempDir::new().unwrap();
    let wrote = seed_bash_catalog(dir.path()).unwrap();
    assert!(wrote);

    let catalog = CorrectionCatalog::load(dir.path()).unwrap();
    let pf = DefaultPreflight::new(catalog);

    let canonical_hits = [
        ("sleep 30 && echo ready", "sleep-chain"),
        ("cat foo.txt | grep bar", "cat-grep"),
        ("echo 'hi there' > out.txt", "echo-redirect"),
        ("git push -f origin main", "force-push"),
        ("export API_KEY='sk-1234567890abcdef'", "inline-secret"),
    ];
    for (cmd, label) in canonical_hits {
        let result = pf.check_invocation("bash", cmd);
        let correction = result
            .err()
            .unwrap_or_else(|| panic!("{label} ({cmd}) should have been blocked"));
        assert!(
            correction.is_blocked(),
            "{label} ({cmd}) should block, got warning: {}",
            correction.render()
        );
    }

    // A benign command must not trigger any seed rule.
    assert!(
        pf.check_invocation("bash", "ls -la /tmp").is_ok(),
        "benign command must not be blocked by seed"
    );
}

#[test]
fn seed_rules_match_the_five_skill_ids() {
    // Regression on the ID set — the skill names these exactly.
    let ids: Vec<_> = bash_seed_rules().into_iter().map(|r| r.id).collect();
    let mut expected = vec![
        "sleep-chain-polling".to_string(),
        "cat-grep-head-for-files".to_string(),
        "echo-to-file-creation".to_string(),
        "git-push-force-to-protected-branch".to_string(),
        "secret-inline-in-command".to_string(),
    ];
    expected.sort();
    let mut actual = ids;
    actual.sort();
    assert_eq!(actual, expected);
}

#[test]
fn hot_reload_via_watcher_picks_up_new_rule() {
    // Write an initial empty catalog, attach the watcher, then drop in a new
    // YAML. The watcher should reload on its own within a few hundred ms.
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("bash").join("active")).unwrap();
    std::fs::write(
        dir.path().join("bash").join("active").join("corrections.yaml"),
        "rules: []\n",
    )
    .unwrap();

    let catalog = CorrectionCatalog::load_and_watch(dir.path()).unwrap();
    let pf = DefaultPreflight::new(catalog.clone());
    assert!(pf.check_invocation("bash", "rm -rf /").is_ok());

    // Now write the new rule.
    write_active(
        dir.path(),
        "bash",
        vec![CorrectionRule::new(
            "rm-root",
            r"^rm\s+-rf\s+/(?:\s|$)",
            "That would wipe the filesystem. Target a specific path.",
            "test",
        )],
    );

    // Poll until the watcher reloads (up to 5 s). The debounce is 250 ms so
    // this normally lands within the first second.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if pf.check_invocation("bash", "rm -rf /").is_err() {
            break;
        }
        if Instant::now() > deadline {
            panic!("watcher did not pick up new rule within 5s");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn per_tool_scoping_keeps_rules_isolated() {
    let dir = TempDir::new().unwrap();
    write_active(
        dir.path(),
        "bash",
        vec![CorrectionRule::new("overbroad", "dangerous", "fix", "seed")],
    );
    write_active(
        dir.path(),
        "http",
        vec![CorrectionRule::new("http-rule", "example.com", "fix", "seed")],
    );
    let catalog = CorrectionCatalog::load(dir.path()).unwrap();
    let pf = DefaultPreflight::new(catalog);

    // bash rule stays in the bash namespace.
    assert!(pf.check_invocation("bash", "dangerous thing").is_err());
    assert!(pf.check_invocation("http", "dangerous thing").is_ok());

    // http rule stays in the http namespace.
    assert!(pf.check_invocation("http", "hit example.com").is_err());
    assert!(pf.check_invocation("bash", "hit example.com").is_ok());
}
