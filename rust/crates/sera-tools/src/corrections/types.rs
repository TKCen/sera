//! Data types for the correction catalog.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The verdict a preflight check returns when a rule matches.
///
/// `Blocked` cancels the tool call and feeds `correction` back to the model.
/// `Warning` lets the call proceed (dispatcher decides whether to annotate
/// the output — kept non-fatal so warnings don't throttle the model).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolCorrection {
    Blocked {
        antipattern: String,
        correction: String,
    },
    Warning {
        suboptimal: String,
        better: String,
    },
}

impl ToolCorrection {
    /// Render as the text the model sees as its tool-result.
    pub fn render(&self) -> String {
        match self {
            Self::Blocked { antipattern, correction } => format!(
                "Error: Blocked — {antipattern}.\n{correction}"
            ),
            Self::Warning { suboptimal, better } => format!(
                "Warning: {suboptimal}. Prefer: {better}"
            ),
        }
    }

    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Blocked { .. })
    }
}

/// How the `pattern` field should be interpreted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    /// Compile `pattern` as a regex (default).
    #[default]
    Regex,
    /// Plain substring match.
    Substring,
    /// Exact string equality on the whole invocation text.
    Exact,
}

/// Severity of a correction rule.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionSeverity {
    /// Cancel the call, return the correction as the tool result.
    #[default]
    Block,
    /// Allow the call but emit the correction as a warning.
    Warn,
}

/// A single correction rule as stored in YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionRule {
    /// Stable identifier used by propose/approve and hit-count updates.
    pub id: String,
    /// The pattern to match against the tool invocation text.
    pub pattern: String,
    /// How `pattern` is interpreted.
    #[serde(default)]
    pub matches: MatchKind,
    /// Severity — `block` cancels, `warn` annotates.
    #[serde(default)]
    pub severity: CorrectionSeverity,
    /// Short human-readable anti-pattern name.
    #[serde(default)]
    pub antipattern: String,
    /// The corrective text emitted to the model on match.
    pub correction: String,
    /// Who added this rule (seed, agent, admin, ...).
    #[serde(default)]
    pub added_by: String,
    /// When the rule was added.
    #[serde(default = "default_now")]
    pub added_at: DateTime<Utc>,
    /// Running count of times this rule has fired in this process.
    ///
    /// Persisted back to YAML on a best-effort basis by the catalog; the
    /// in-memory copy is authoritative for the live process.
    #[serde(default)]
    pub hit_count: u64,
    /// Timestamp of the most recent match.
    #[serde(default)]
    pub last_hit: Option<DateTime<Utc>>,
}

impl CorrectionRule {
    /// Build a rule from bare fields, filling in sensible defaults.
    pub fn new(
        id: impl Into<String>,
        pattern: impl Into<String>,
        correction: impl Into<String>,
        added_by: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            pattern: pattern.into(),
            matches: MatchKind::Regex,
            severity: CorrectionSeverity::Block,
            antipattern: String::new(),
            correction: correction.into(),
            added_by: added_by.into(),
            added_at: Utc::now(),
            hit_count: 0,
            last_hit: None,
        }
    }

    /// Compute the appropriate `ToolCorrection` for this rule. Cloning is
    /// cheap compared to the tool call it just prevented.
    pub fn to_correction(&self) -> ToolCorrection {
        let antipattern = if self.antipattern.is_empty() {
            self.id.clone()
        } else {
            self.antipattern.clone()
        };
        match self.severity {
            CorrectionSeverity::Block => ToolCorrection::Blocked {
                antipattern,
                correction: self.correction.clone(),
            },
            CorrectionSeverity::Warn => ToolCorrection::Warning {
                suboptimal: antipattern,
                better: self.correction.clone(),
            },
        }
    }
}

fn default_now() -> DateTime<Utc> {
    Utc::now()
}

/// On-disk file shape. Top-level document is `{ rules: [...] }` so future
/// fields (version, metadata) can be added without breaking old catalogs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CorrectionFile {
    #[serde(default)]
    pub rules: Vec<CorrectionRule>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_renders_with_correction() {
        let c = ToolCorrection::Blocked {
            antipattern: "sleep-chain".into(),
            correction: "Use until-loop".into(),
        };
        let text = c.render();
        assert!(text.contains("Blocked"));
        assert!(text.contains("sleep-chain"));
        assert!(text.contains("until-loop"));
    }

    #[test]
    fn warning_renders_non_fatally() {
        let c = ToolCorrection::Warning {
            suboptimal: "cat | grep".into(),
            better: "read_file".into(),
        };
        let text = c.render();
        assert!(text.contains("Warning"));
        assert!(!text.contains("Blocked"));
    }

    #[test]
    fn rule_defaults_regex_and_block() {
        let yaml = r#"
id: sample
pattern: "foo"
correction: "use bar"
"#;
        let rule: CorrectionRule = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(rule.matches, MatchKind::Regex);
        assert_eq!(rule.severity, CorrectionSeverity::Block);
        assert_eq!(rule.hit_count, 0);
    }

    #[test]
    fn rule_to_correction_blocks_by_default() {
        let rule = CorrectionRule::new("r1", "foo", "do bar", "seed");
        assert!(matches!(rule.to_correction(), ToolCorrection::Blocked { .. }));
    }

    #[test]
    fn file_round_trip_preserves_rules() {
        let file = CorrectionFile {
            rules: vec![
                CorrectionRule::new("r1", "pat1", "corr1", "seed"),
                CorrectionRule::new("r2", "pat2", "corr2", "agent"),
            ],
        };
        let yaml = serde_yaml::to_string(&file).unwrap();
        let back: CorrectionFile = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.rules.len(), 2);
        assert_eq!(back.rules[0].id, "r1");
        assert_eq!(back.rules[1].added_by, "agent");
    }
}
