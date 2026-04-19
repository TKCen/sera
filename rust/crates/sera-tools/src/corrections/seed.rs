//! Canonical seed rules from the `tool-layer-reinforcement` skill.
//!
//! Written to `<root>/bash/active/corrections.yaml` on first boot if the
//! file does not yet exist. Rewriting an existing file would clobber
//! hand-tuned rules, so the seeder is strictly idempotent — callers that
//! want a pristine seed must delete the file first.

use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use tracing::info;

use super::types::{CorrectionFile, CorrectionRule, CorrectionSeverity, MatchKind};

/// Default location for the per-user catalog (`~/.sera/tool-corrections/`).
///
/// Falls back to the system temp dir when the home directory can't be
/// resolved — the runtime still starts, rules just won't persist.
pub fn default_root() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        home.join(".sera").join("tool-corrections")
    } else {
        std::env::temp_dir().join("sera-tool-corrections")
    }
}

/// The canonical bash anti-patterns from the skill. Keep this list small and
/// high-signal — every added rule costs every bash call a regex match.
pub fn bash_seed_rules() -> Vec<CorrectionRule> {
    let now = Utc::now();
    let mut rules = vec![
        CorrectionRule {
            id: "sleep-chain-polling".to_string(),
            antipattern: "sleep N && cmd polling".to_string(),
            pattern: r"\bsleep\s+\d+\s*&&".to_string(),
            matches: MatchKind::Regex,
            severity: CorrectionSeverity::Block,
            correction:
                "Use `until <check>; do sleep 2; done` for condition polling, or start the \
                 long-running command with run_in_background and await completion via Monitor. \
                 Chained sleeps burn cache and waste wall time."
                    .to_string(),
            added_by: "seed".to_string(),
            added_at: now,
            hit_count: 0,
            last_hit: None,
        },
        CorrectionRule {
            id: "cat-grep-head-for-files".to_string(),
            antipattern: "cat | grep | head on a file".to_string(),
            // Match bash pipelines that open a file with cat and then filter.
            // Deliberately anchored to `cat ` so we don't catch unrelated cats.
            pattern: r"\bcat\s+[^|]*\|\s*(grep|head|tail|sed|awk)\b".to_string(),
            matches: MatchKind::Regex,
            severity: CorrectionSeverity::Block,
            correction:
                "Use the read_file tool with offset/limit, or the grep tool for content search. \
                 Piping cat into grep/head/tail/sed/awk is the inefficient shell idiom."
                    .to_string(),
            added_by: "seed".to_string(),
            added_at: now,
            hit_count: 0,
            last_hit: None,
        },
        CorrectionRule {
            id: "echo-to-file-creation".to_string(),
            antipattern: "echo/heredoc into a file".to_string(),
            // Covers `echo "..." > path`, `printf "..." > path`, and
            // heredoc-style `cat <<EOF > path`.
            pattern:
                r#"(?:\becho\b[^>]*>\s*\S+|\bprintf\b[^>]*>\s*\S+|\bcat\s*<<[-']?\w+)"#
                    .to_string(),
            matches: MatchKind::Regex,
            severity: CorrectionSeverity::Block,
            correction:
                "Use the write_file tool to create or overwrite files. Shell redirection misses \
                 the audit trail and escapes trip up on special characters."
                    .to_string(),
            added_by: "seed".to_string(),
            added_at: now,
            hit_count: 0,
            last_hit: None,
        },
        CorrectionRule {
            id: "git-push-force-to-protected-branch".to_string(),
            antipattern: "force-push to a protected branch".to_string(),
            pattern:
                r"\bgit\s+push\s+(?:-f|--force|--force-with-lease)\b[^\n]*\b(?:main|master|release|prod|production)\b"
                    .to_string(),
            matches: MatchKind::Regex,
            severity: CorrectionSeverity::Block,
            correction:
                "Force-pushing to a protected branch rewrites shared history. Open a pull \
                 request against the detected base branch instead."
                    .to_string(),
            added_by: "seed".to_string(),
            added_at: now,
            hit_count: 0,
            last_hit: None,
        },
        CorrectionRule {
            id: "secret-inline-in-command".to_string(),
            antipattern: "secret/token/apikey pasted inline into a command".to_string(),
            // Catch export FOO=… / TOKEN=… / api_key=… with a quoted value,
            // or common CLI flags with an inline token value.
            pattern:
                r#"(?i)(?:^|\s)(?:export\s+)?(?:secret|token|api[_-]?key|password|passwd)\s*=\s*(?:['"][^'"]{4,}['"]|\S{12,})"#
                    .to_string(),
            matches: MatchKind::Regex,
            severity: CorrectionSeverity::Block,
            correction:
                "Do not paste secret values into commands. Reference the secret by its file \
                 path or environment variable name; the Secrets Manager injects it at execution \
                 time. Inline values end up in shell history and audit logs."
                    .to_string(),
            added_by: "seed".to_string(),
            added_at: now,
            hit_count: 0,
            last_hit: None,
        },
    ];
    // Stable ordering so reload is deterministic.
    rules.sort_by(|a, b| a.id.cmp(&b.id));
    rules
}

/// Write the default bash YAML if it does not already exist. Returns `true`
/// when a fresh seed was written, `false` if one was already present.
pub fn seed_bash_catalog(root: &Path) -> io::Result<bool> {
    let dir = root.join("bash").join("active");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("corrections.yaml");
    if path.exists() {
        return Ok(false);
    }
    let file = CorrectionFile { rules: bash_seed_rules() };
    let yaml = serde_yaml::to_string(&file)
        .map_err(|e| io::Error::other(format!("serialize seed: {e}")))?;
    std::fs::write(&path, yaml)?;
    info!(path = %path.display(), "seeded bash correction catalog");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;
    use tempfile::TempDir;

    #[test]
    fn every_seed_pattern_compiles() {
        for rule in bash_seed_rules() {
            assert_eq!(rule.matches, MatchKind::Regex, "seed rules are regex");
            Regex::new(&rule.pattern)
                .unwrap_or_else(|e| panic!("seed rule '{}' bad regex: {e}", rule.id));
        }
    }

    #[test]
    fn sleep_chain_rule_matches_canonical_case() {
        let rule = bash_seed_rules()
            .into_iter()
            .find(|r| r.id == "sleep-chain-polling")
            .unwrap();
        let re = Regex::new(&rule.pattern).unwrap();
        assert!(re.is_match("sleep 30 && gh pr checks 950"));
        assert!(re.is_match("  sleep 2 && echo hi"));
        // Plain `sleep 30` without a chained command is fine.
        assert!(!re.is_match("sleep 30"));
    }

    #[test]
    fn cat_grep_rule_matches_shell_idiom() {
        let rule = bash_seed_rules()
            .into_iter()
            .find(|r| r.id == "cat-grep-head-for-files")
            .unwrap();
        let re = Regex::new(&rule.pattern).unwrap();
        assert!(re.is_match("cat foo.txt | grep bar"));
        assert!(re.is_match("cat file | head -n 5"));
        // A bare `cat foo.txt` is fine.
        assert!(!re.is_match("cat foo.txt"));
    }

    #[test]
    fn echo_rule_matches_redirection_and_heredoc() {
        let rule = bash_seed_rules()
            .into_iter()
            .find(|r| r.id == "echo-to-file-creation")
            .unwrap();
        let re = Regex::new(&rule.pattern).unwrap();
        assert!(re.is_match("echo 'hi' > out.txt"));
        assert!(re.is_match("printf '%s' data > out"));
        assert!(re.is_match("cat <<EOF > out"));
        assert!(!re.is_match("echo hi"));
    }

    #[test]
    fn force_push_rule_matches_protected_branches() {
        let rule = bash_seed_rules()
            .into_iter()
            .find(|r| r.id == "git-push-force-to-protected-branch")
            .unwrap();
        let re = Regex::new(&rule.pattern).unwrap();
        assert!(re.is_match("git push -f origin main"));
        assert!(re.is_match("git push --force origin master"));
        assert!(re.is_match("git push --force-with-lease origin production"));
        // A feature branch is fine.
        assert!(!re.is_match("git push -f origin feature/foo"));
    }

    #[test]
    fn secret_rule_matches_inline_values() {
        let rule = bash_seed_rules()
            .into_iter()
            .find(|r| r.id == "secret-inline-in-command")
            .unwrap();
        let re = Regex::new(&rule.pattern).unwrap();
        assert!(re.is_match("export API_KEY=\"sk-1234567890abcdef\""));
        assert!(re.is_match("TOKEN=xoxp-1234567890-0987654321-abcdef"));
        assert!(re.is_match("password='hunter22-hunter22'"));
        // Placeholder or env reference is fine.
        assert!(!re.is_match("export API_KEY=$API_KEY"));
    }

    #[test]
    fn seed_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let wrote_first = seed_bash_catalog(dir.path()).unwrap();
        assert!(wrote_first);
        let wrote_second = seed_bash_catalog(dir.path()).unwrap();
        assert!(!wrote_second, "seed must not clobber an existing catalog");
    }
}
