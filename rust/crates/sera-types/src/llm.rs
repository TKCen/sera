//! Unified LLM reasoning / thinking abstraction.
//!
//! [`ThinkingLevel`] is the provider-agnostic reasoning intensity knob.
//! Higher levels instruct the chosen model to invest more tokens in
//! step-by-step reasoning before producing a final answer.
//!
//! Provider translations (do not modify wire requests here — see
//! `sera-models::thinking` for that layer):
//!
//! | Level   | Anthropic effort | OpenAI reasoningEffort | Gemini budgetTokens |
//! |---------|-----------------|------------------------|---------------------|
//! | None    | (omitted)       | (omitted)              | disabled            |
//! | Low     | "low"           | "low"                  | 512                 |
//! | Medium  | "medium"        | "medium"                | 2 048               |
//! | High    | "high"          | "high"                  | 8 192               |
//! | XHigh   | "high" + budget | "high"                  | 32 768              |

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

// ── ThinkingLevel ─────────────────────────────────────────────────────────────

/// Provider-agnostic reasoning intensity for an LLM request.
///
/// Default is [`ThinkingLevel::None`] — no reasoning overhead, fastest
/// time-to-first-token.  Higher levels trade latency and token cost for
/// improved accuracy on complex tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingLevel {
    /// No reasoning — omit the provider-native reasoning parameter entirely.
    #[default]
    None,
    /// Lightweight reasoning pass (low latency, minimal token overhead).
    Low,
    /// Balanced reasoning pass (recommended default when reasoning is needed).
    Medium,
    /// Deep reasoning pass (higher latency, better accuracy on hard tasks).
    High,
    /// Extended reasoning pass (maximum budget; use sparingly).
    XHigh,
}

impl ThinkingLevel {
    /// Returns `true` if reasoning is disabled.
    pub fn is_none(&self) -> bool {
        matches!(self, ThinkingLevel::None)
    }

    /// Anthropic `effort` string for this level.
    ///
    /// Returns `None` when the level is [`ThinkingLevel::None`] — callers
    /// should omit the field entirely in that case.
    pub fn to_anthropic_effort(self) -> Option<&'static str> {
        match self {
            ThinkingLevel::None => None,
            ThinkingLevel::Low => Some("low"),
            ThinkingLevel::Medium => Some("medium"),
            ThinkingLevel::High | ThinkingLevel::XHigh => Some("high"),
        }
    }

    /// OpenAI `reasoningEffort` string for this level.
    ///
    /// Returns `None` when the level is [`ThinkingLevel::None`].
    pub fn to_openai_reasoning_effort(self) -> Option<&'static str> {
        match self {
            ThinkingLevel::None => None,
            ThinkingLevel::Low => Some("low"),
            ThinkingLevel::Medium => Some("medium"),
            ThinkingLevel::High | ThinkingLevel::XHigh => Some("high"),
        }
    }

    /// Google Gemini `thinking.budgetTokens` value for this level.
    ///
    /// Returns `None` when the level is [`ThinkingLevel::None`], signalling
    /// that `thinking.enabled` should be set to `false`.
    pub fn to_gemini_budget_tokens(self) -> Option<u32> {
        match self {
            ThinkingLevel::None => None,
            ThinkingLevel::Low => Some(512),
            ThinkingLevel::Medium => Some(2_048),
            ThinkingLevel::High => Some(8_192),
            ThinkingLevel::XHigh => Some(32_768),
        }
    }
}

// ── Display ───────────────────────────────────────────────────────────────────

impl fmt::Display for ThinkingLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ThinkingLevel::None => "none",
            ThinkingLevel::Low => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High => "high",
            ThinkingLevel::XHigh => "xhigh",
        };
        f.write_str(s)
    }
}

// ── FromStr ───────────────────────────────────────────────────────────────────

/// Error returned when a string cannot be parsed as a [`ThinkingLevel`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseThinkingLevelError(String);

impl fmt::Display for ParseThinkingLevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown thinking level {:?}; expected one of: none, low, medium, high, xhigh",
            self.0
        )
    }
}

impl std::error::Error for ParseThinkingLevelError {}

impl FromStr for ThinkingLevel {
    type Err = ParseThinkingLevelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "none" => Ok(ThinkingLevel::None),
            "low" => Ok(ThinkingLevel::Low),
            "medium" => Ok(ThinkingLevel::Medium),
            "high" => Ok(ThinkingLevel::High),
            "xhigh" | "x_high" => Ok(ThinkingLevel::XHigh),
            other => Err(ParseThinkingLevelError(other.to_string())),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Default ───────────────────────────────────────────────────────────────

    #[test]
    fn default_is_none() {
        assert_eq!(ThinkingLevel::default(), ThinkingLevel::None);
        assert!(ThinkingLevel::None.is_none());
    }

    // ── Serde roundtrip ───────────────────────────────────────────────────────

    #[test]
    fn serde_roundtrip_all_variants() {
        for level in [
            ThinkingLevel::None,
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
            ThinkingLevel::XHigh,
        ] {
            let json = serde_json::to_string(&level).expect("serialize");
            let parsed: ThinkingLevel = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(parsed, level, "roundtrip failed for {level}");
        }
    }

    #[test]
    fn serde_none_serializes_to_none_string() {
        let json = serde_json::to_string(&ThinkingLevel::None).unwrap();
        assert_eq!(json, "\"none\"");
    }

    #[test]
    fn serde_xhigh_serializes_to_xhigh_string() {
        let json = serde_json::to_string(&ThinkingLevel::XHigh).unwrap();
        assert_eq!(json, "\"x_high\"");
    }

    // ── Display ───────────────────────────────────────────────────────────────

    #[test]
    fn display_all_variants() {
        assert_eq!(ThinkingLevel::None.to_string(), "none");
        assert_eq!(ThinkingLevel::Low.to_string(), "low");
        assert_eq!(ThinkingLevel::Medium.to_string(), "medium");
        assert_eq!(ThinkingLevel::High.to_string(), "high");
        assert_eq!(ThinkingLevel::XHigh.to_string(), "xhigh");
    }

    // ── FromStr ───────────────────────────────────────────────────────────────

    #[test]
    fn from_str_all_variants() {
        assert_eq!("none".parse::<ThinkingLevel>().unwrap(), ThinkingLevel::None);
        assert_eq!("low".parse::<ThinkingLevel>().unwrap(), ThinkingLevel::Low);
        assert_eq!("medium".parse::<ThinkingLevel>().unwrap(), ThinkingLevel::Medium);
        assert_eq!("high".parse::<ThinkingLevel>().unwrap(), ThinkingLevel::High);
        assert_eq!("xhigh".parse::<ThinkingLevel>().unwrap(), ThinkingLevel::XHigh);
        assert_eq!("x_high".parse::<ThinkingLevel>().unwrap(), ThinkingLevel::XHigh);
    }

    #[test]
    fn from_str_case_insensitive() {
        assert_eq!("NONE".parse::<ThinkingLevel>().unwrap(), ThinkingLevel::None);
        assert_eq!("HIGH".parse::<ThinkingLevel>().unwrap(), ThinkingLevel::High);
        assert_eq!("XHigh".parse::<ThinkingLevel>().unwrap(), ThinkingLevel::XHigh);
        assert_eq!("Medium".parse::<ThinkingLevel>().unwrap(), ThinkingLevel::Medium);
    }

    #[test]
    fn from_str_rejects_garbage() {
        let err = "superfast".parse::<ThinkingLevel>().unwrap_err();
        assert!(err.to_string().contains("superfast"));

        let err2 = "".parse::<ThinkingLevel>().unwrap_err();
        assert!(err2.to_string().contains("unknown thinking level"));

        let err3 = "turbo".parse::<ThinkingLevel>().unwrap_err();
        assert!(err3.to_string().contains("turbo"));
    }

    // ── Provider mapping matrix ───────────────────────────────────────────────

    #[test]
    fn anthropic_effort_mapping() {
        assert_eq!(ThinkingLevel::None.to_anthropic_effort(), None);
        assert_eq!(ThinkingLevel::Low.to_anthropic_effort(), Some("low"));
        assert_eq!(ThinkingLevel::Medium.to_anthropic_effort(), Some("medium"));
        assert_eq!(ThinkingLevel::High.to_anthropic_effort(), Some("high"));
        // XHigh also maps to "high" — budget is controlled separately
        assert_eq!(ThinkingLevel::XHigh.to_anthropic_effort(), Some("high"));
    }

    #[test]
    fn openai_reasoning_effort_mapping() {
        assert_eq!(ThinkingLevel::None.to_openai_reasoning_effort(), None);
        assert_eq!(ThinkingLevel::Low.to_openai_reasoning_effort(), Some("low"));
        assert_eq!(ThinkingLevel::Medium.to_openai_reasoning_effort(), Some("medium"));
        assert_eq!(ThinkingLevel::High.to_openai_reasoning_effort(), Some("high"));
        assert_eq!(ThinkingLevel::XHigh.to_openai_reasoning_effort(), Some("high"));
    }

    #[test]
    fn gemini_budget_tokens_mapping() {
        assert_eq!(ThinkingLevel::None.to_gemini_budget_tokens(), None);
        assert_eq!(ThinkingLevel::Low.to_gemini_budget_tokens(), Some(512));
        assert_eq!(ThinkingLevel::Medium.to_gemini_budget_tokens(), Some(2_048));
        assert_eq!(ThinkingLevel::High.to_gemini_budget_tokens(), Some(8_192));
        assert_eq!(ThinkingLevel::XHigh.to_gemini_budget_tokens(), Some(32_768));
    }

    // ── is_none ───────────────────────────────────────────────────────────────

    #[test]
    fn is_none_only_for_none_variant() {
        assert!(ThinkingLevel::None.is_none());
        assert!(!ThinkingLevel::Low.is_none());
        assert!(!ThinkingLevel::Medium.is_none());
        assert!(!ThinkingLevel::High.is_none());
        assert!(!ThinkingLevel::XHigh.is_none());
    }
}
