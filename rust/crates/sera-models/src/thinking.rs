//! Unified thinking / reasoning abstraction across providers.
//!
//! Different providers expose reasoning or "thinking" mode differently:
//! - **Qwen**: `enable_thinking: true` + separate `thinking` field in responses
//! - **DeepSeek** and **OpenAI o1/o3**: `reasoning_effort: "low" | "medium" | "high"`
//! - **Anthropic Claude**: extended-thinking block `thinking: { type: "enabled", budget_tokens: N }`
//!
//! [`ThinkingConfig`] expresses a provider-agnostic level. Each
//! [`ProviderKind`] knows how to render it into a native JSON request field
//! via [`ThinkingConfig::apply_to_body`].

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ReasoningLevel
// ---------------------------------------------------------------------------

/// Provider-agnostic reasoning intensity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningLevel {
    /// No reasoning — omit the provider-native reasoning parameter entirely.
    #[default]
    Off,
    Low,
    Medium,
    High,
}

impl ReasoningLevel {
    /// Provider-native effort string ("low" / "medium" / "high") for
    /// OpenAI-family / DeepSeek. Returns `None` for `Off`.
    pub fn as_effort_str(self) -> Option<&'static str> {
        match self {
            ReasoningLevel::Off => None,
            ReasoningLevel::Low => Some("low"),
            ReasoningLevel::Medium => Some("medium"),
            ReasoningLevel::High => Some("high"),
        }
    }

    /// Default Claude thinking budget for each level (only used when
    /// `ThinkingConfig::budget_tokens` is `None`).
    pub fn default_claude_budget(self) -> Option<u32> {
        match self {
            ReasoningLevel::Off => None,
            ReasoningLevel::Low => Some(1_000),
            ReasoningLevel::Medium => Some(5_000),
            ReasoningLevel::High => Some(15_000),
        }
    }
}

// ---------------------------------------------------------------------------
// ProviderKind
// ---------------------------------------------------------------------------

/// Which provider wire format to emit when applying a thinking config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// OpenAI o1/o3 reasoning-effort (`reasoning.effort`).
    OpenAi,
    /// DeepSeek R1 / reasoner models (`reasoning_effort`).
    DeepSeek,
    /// Qwen chat (`enable_thinking` boolean).
    Qwen,
    /// Anthropic Claude extended thinking (`thinking.type/budget_tokens`).
    Anthropic,
    /// Google Gemini thinking (`thinking.enabled` + `thinking.budgetTokens`).
    GoogleAi,
    /// Unknown / provider without a reasoning field — no-op.
    Generic,
}

impl ProviderKind {
    /// Infer the provider kind from a free-text provider id (case-insensitive
    /// substring match).  Falls back to [`ProviderKind::Generic`].
    pub fn infer(provider_id: &str) -> Self {
        let lower = provider_id.to_ascii_lowercase();
        if lower.contains("anthropic") || lower.contains("claude") {
            ProviderKind::Anthropic
        } else if lower.contains("deepseek") {
            ProviderKind::DeepSeek
        } else if lower.contains("qwen") || lower.contains("alibaba") {
            ProviderKind::Qwen
        } else if lower.contains("openai")
            || lower.contains("gpt")
            || lower.contains("o1")
            || lower.contains("o3")
            || lower.contains("o4")
        {
            ProviderKind::OpenAi
        } else if lower.contains("google")
            || lower.contains("gemini")
            || lower.contains("googleai")
        {
            ProviderKind::GoogleAi
        } else {
            ProviderKind::Generic
        }
    }
}

// ---------------------------------------------------------------------------
// ThinkingConfig
// ---------------------------------------------------------------------------

/// Unified reasoning configuration threaded through every LLM request.
///
/// Defaults to [`ReasoningLevel::Off`] with no explicit token budget; when
/// the provider does not support reasoning, the config is a no-op.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThinkingConfig {
    #[serde(default)]
    pub level: ReasoningLevel,
    /// Optional provider-specific cap (only honoured by Claude today).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u32>,
}

impl ThinkingConfig {
    pub const OFF: ThinkingConfig = ThinkingConfig {
        level: ReasoningLevel::Off,
        budget_tokens: None,
    };

    pub fn new(level: ReasoningLevel) -> Self {
        Self {
            level,
            budget_tokens: None,
        }
    }

    pub fn with_budget(mut self, budget_tokens: u32) -> Self {
        self.budget_tokens = Some(budget_tokens);
        self
    }

    pub fn is_off(&self) -> bool {
        matches!(self.level, ReasoningLevel::Off)
    }

    /// Mutate the provider request JSON body in place so the provider-native
    /// reasoning/thinking parameter is set according to this config.
    ///
    /// When [`ReasoningLevel::Off`], the body is not modified for any
    /// provider that uses an "effort string" field (OpenAI, DeepSeek).  For
    /// Qwen, `enable_thinking` is set to `false` explicitly so that the
    /// provider toggles thinking off reliably.  For Anthropic, the
    /// `thinking` block is omitted entirely.
    pub fn apply_to_body(&self, body: &mut serde_json::Value, provider: ProviderKind) {
        let obj = match body.as_object_mut() {
            Some(o) => o,
            None => return,
        };

        match provider {
            ProviderKind::OpenAi => {
                if let Some(effort) = self.level.as_effort_str() {
                    obj.insert(
                        "reasoning".to_string(),
                        serde_json::json!({ "effort": effort }),
                    );
                }
            }
            ProviderKind::DeepSeek => {
                if let Some(effort) = self.level.as_effort_str() {
                    obj.insert(
                        "reasoning_effort".to_string(),
                        serde_json::Value::String(effort.to_string()),
                    );
                }
            }
            ProviderKind::Qwen => {
                obj.insert(
                    "enable_thinking".to_string(),
                    serde_json::Value::Bool(!self.is_off()),
                );
            }
            ProviderKind::Anthropic => {
                if self.is_off() {
                    return;
                }
                let budget = self
                    .budget_tokens
                    .or_else(|| self.level.default_claude_budget())
                    .unwrap_or(5_000);
                obj.insert(
                    "thinking".to_string(),
                    serde_json::json!({
                        "type": "enabled",
                        "budget_tokens": budget,
                    }),
                );
            }
            ProviderKind::GoogleAi => {
                // Google Gemini: `thinking.enabled` boolean + `thinking.budgetTokens`.
                // When Off, explicitly disable so the model does not infer its own budget.
                let enabled = !self.is_off();
                let default_budget = match self.level {
                    ReasoningLevel::Off => None,
                    ReasoningLevel::Low => Some(512),
                    ReasoningLevel::Medium => Some(2_048),
                    ReasoningLevel::High => Some(8_192),
                };
                let budget = if enabled {
                    self.budget_tokens.or(default_budget)
                } else {
                    None
                };
                let mut thinking_obj = serde_json::json!({ "enabled": enabled });
                if let Some(b) = budget {
                    thinking_obj["budgetTokens"] = serde_json::json!(b);
                }
                obj.insert("thinking".to_string(), thinking_obj);
            }
            ProviderKind::Generic => {
                // No-op — provider does not expose a reasoning field.
                tracing::debug!("provider does not support thinking levels");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── ProviderKind::infer ──────────────────────────────────────────────────

    #[test]
    fn infer_openai_family() {
        assert_eq!(ProviderKind::infer("openai"), ProviderKind::OpenAi);
        assert_eq!(ProviderKind::infer("openai-o1"), ProviderKind::OpenAi);
        assert_eq!(ProviderKind::infer("gpt-4o"), ProviderKind::OpenAi);
        assert_eq!(ProviderKind::infer("o3-mini"), ProviderKind::OpenAi);
    }

    #[test]
    fn infer_anthropic_family() {
        assert_eq!(ProviderKind::infer("anthropic"), ProviderKind::Anthropic);
        assert_eq!(ProviderKind::infer("claude-3-5-sonnet"), ProviderKind::Anthropic);
    }

    #[test]
    fn infer_deepseek() {
        assert_eq!(ProviderKind::infer("deepseek-r1"), ProviderKind::DeepSeek);
        assert_eq!(ProviderKind::infer("DeepSeek"), ProviderKind::DeepSeek);
    }

    #[test]
    fn infer_qwen() {
        assert_eq!(ProviderKind::infer("qwen-max"), ProviderKind::Qwen);
        assert_eq!(ProviderKind::infer("alibaba-qwen"), ProviderKind::Qwen);
    }

    #[test]
    fn infer_generic_fallback() {
        assert_eq!(ProviderKind::infer("mistral"), ProviderKind::Generic);
        assert_eq!(ProviderKind::infer("local-lmstudio"), ProviderKind::Generic);
        assert_eq!(ProviderKind::infer(""), ProviderKind::Generic);
    }

    // ── apply_to_body: OpenAI ────────────────────────────────────────────────

    #[test]
    fn openai_off_does_not_touch_body() {
        let mut body = json!({"model": "o1", "messages": []});
        ThinkingConfig::OFF.apply_to_body(&mut body, ProviderKind::OpenAi);
        assert!(body.get("reasoning").is_none());
    }

    #[test]
    fn openai_low_medium_high_map_to_effort_strings() {
        for (level, expected) in [
            (ReasoningLevel::Low, "low"),
            (ReasoningLevel::Medium, "medium"),
            (ReasoningLevel::High, "high"),
        ] {
            let mut body = json!({});
            ThinkingConfig::new(level).apply_to_body(&mut body, ProviderKind::OpenAi);
            assert_eq!(
                body["reasoning"]["effort"],
                expected,
                "OpenAI mapping for {level:?} should be {expected}"
            );
        }
    }

    // ── apply_to_body: DeepSeek ──────────────────────────────────────────────

    #[test]
    fn deepseek_off_does_not_touch_body() {
        let mut body = json!({});
        ThinkingConfig::OFF.apply_to_body(&mut body, ProviderKind::DeepSeek);
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn deepseek_levels_map_to_effort_strings() {
        for (level, expected) in [
            (ReasoningLevel::Low, "low"),
            (ReasoningLevel::Medium, "medium"),
            (ReasoningLevel::High, "high"),
        ] {
            let mut body = json!({});
            ThinkingConfig::new(level).apply_to_body(&mut body, ProviderKind::DeepSeek);
            assert_eq!(body["reasoning_effort"], expected);
        }
    }

    // ── apply_to_body: Qwen ─────────────────────────────────────────────────

    #[test]
    fn qwen_off_sets_enable_thinking_false() {
        let mut body = json!({});
        ThinkingConfig::OFF.apply_to_body(&mut body, ProviderKind::Qwen);
        assert_eq!(body["enable_thinking"], false);
    }

    #[test]
    fn qwen_any_nonoff_sets_enable_thinking_true() {
        for level in [
            ReasoningLevel::Low,
            ReasoningLevel::Medium,
            ReasoningLevel::High,
        ] {
            let mut body = json!({});
            ThinkingConfig::new(level).apply_to_body(&mut body, ProviderKind::Qwen);
            assert_eq!(
                body["enable_thinking"], true,
                "Qwen should enable thinking for {level:?}"
            );
        }
    }

    // ── apply_to_body: Anthropic ────────────────────────────────────────────

    #[test]
    fn anthropic_off_omits_thinking_block() {
        let mut body = json!({"model": "claude-3-5-sonnet"});
        ThinkingConfig::OFF.apply_to_body(&mut body, ProviderKind::Anthropic);
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn anthropic_uses_default_budgets_per_level() {
        for (level, expected_budget) in [
            (ReasoningLevel::Low, 1_000u32),
            (ReasoningLevel::Medium, 5_000u32),
            (ReasoningLevel::High, 15_000u32),
        ] {
            let mut body = json!({});
            ThinkingConfig::new(level).apply_to_body(&mut body, ProviderKind::Anthropic);
            assert_eq!(body["thinking"]["type"], "enabled");
            assert_eq!(
                body["thinking"]["budget_tokens"],
                serde_json::json!(expected_budget),
                "claude budget mismatch for {level:?}"
            );
        }
    }

    #[test]
    fn anthropic_explicit_budget_overrides_default() {
        let mut body = json!({});
        ThinkingConfig::new(ReasoningLevel::Low)
            .with_budget(7_777)
            .apply_to_body(&mut body, ProviderKind::Anthropic);
        assert_eq!(body["thinking"]["budget_tokens"], serde_json::json!(7_777u32));
    }

    // ── apply_to_body: Generic (no-op) ───────────────────────────────────────

    #[test]
    fn generic_provider_is_noop() {
        let mut body = json!({"model": "mistral", "messages": []});
        let original = body.clone();
        ThinkingConfig::new(ReasoningLevel::High).apply_to_body(&mut body, ProviderKind::Generic);
        assert_eq!(body, original);
    }

    #[test]
    fn apply_to_non_object_body_is_noop() {
        let mut body = json!(["not", "an", "object"]);
        let original = body.clone();
        ThinkingConfig::new(ReasoningLevel::High).apply_to_body(&mut body, ProviderKind::OpenAi);
        assert_eq!(body, original);
    }

    // ── Serde round-trips ────────────────────────────────────────────────────

    #[test]
    fn reasoning_level_serde_roundtrip() {
        for level in [
            ReasoningLevel::Off,
            ReasoningLevel::Low,
            ReasoningLevel::Medium,
            ReasoningLevel::High,
        ] {
            let s = serde_json::to_string(&level).unwrap();
            let parsed: ReasoningLevel = serde_json::from_str(&s).unwrap();
            assert_eq!(parsed, level);
        }
    }

    #[test]
    fn thinking_config_default_is_off() {
        let cfg = ThinkingConfig::default();
        assert_eq!(cfg.level, ReasoningLevel::Off);
        assert!(cfg.is_off());
        assert!(cfg.budget_tokens.is_none());
    }

    #[test]
    fn thinking_config_serde_omits_budget_when_none() {
        let cfg = ThinkingConfig::new(ReasoningLevel::High);
        let s = serde_json::to_string(&cfg).unwrap();
        assert!(!s.contains("budget_tokens"), "should omit when None: {s}");
    }

    #[test]
    fn thinking_config_serde_roundtrip_with_budget() {
        let cfg = ThinkingConfig::new(ReasoningLevel::Medium).with_budget(2500);
        let s = serde_json::to_string(&cfg).unwrap();
        let parsed: ThinkingConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.level, ReasoningLevel::Medium);
        assert_eq!(parsed.budget_tokens, Some(2500));
    }

    // ── apply_to_body: GoogleAi ──────────────────────────────────────────────

    #[test]
    fn google_ai_off_sets_enabled_false_and_no_budget() {
        let mut body = json!({"model": "gemini-2.0-flash"});
        ThinkingConfig::OFF.apply_to_body(&mut body, ProviderKind::GoogleAi);
        assert_eq!(body["thinking"]["enabled"], false);
        assert!(body["thinking"].get("budgetTokens").is_none());
    }

    #[test]
    fn google_ai_low_sets_enabled_true_and_512_budget() {
        let mut body = json!({});
        ThinkingConfig::new(ReasoningLevel::Low).apply_to_body(&mut body, ProviderKind::GoogleAi);
        assert_eq!(body["thinking"]["enabled"], true);
        assert_eq!(body["thinking"]["budgetTokens"], serde_json::json!(512u32));
    }

    #[test]
    fn google_ai_medium_sets_2048_budget() {
        let mut body = json!({});
        ThinkingConfig::new(ReasoningLevel::Medium).apply_to_body(&mut body, ProviderKind::GoogleAi);
        assert_eq!(body["thinking"]["enabled"], true);
        assert_eq!(body["thinking"]["budgetTokens"], serde_json::json!(2_048u32));
    }

    #[test]
    fn google_ai_high_sets_8192_budget() {
        let mut body = json!({});
        ThinkingConfig::new(ReasoningLevel::High).apply_to_body(&mut body, ProviderKind::GoogleAi);
        assert_eq!(body["thinking"]["enabled"], true);
        assert_eq!(body["thinking"]["budgetTokens"], serde_json::json!(8_192u32));
    }

    #[test]
    fn google_ai_explicit_budget_overrides_default() {
        let mut body = json!({});
        ThinkingConfig::new(ReasoningLevel::Low)
            .with_budget(9_999)
            .apply_to_body(&mut body, ProviderKind::GoogleAi);
        assert_eq!(body["thinking"]["enabled"], true);
        assert_eq!(body["thinking"]["budgetTokens"], serde_json::json!(9_999u32));
    }

    // ── ProviderKind::infer — GoogleAi family ────────────────────────────────

    #[test]
    fn infer_google_ai_family() {
        assert_eq!(ProviderKind::infer("gemini-2.0-flash"), ProviderKind::GoogleAi);
        assert_eq!(ProviderKind::infer("google-gemini-pro"), ProviderKind::GoogleAi);
        assert_eq!(ProviderKind::infer("googleai"), ProviderKind::GoogleAi);
        assert_eq!(ProviderKind::infer("GEMINI"), ProviderKind::GoogleAi);
    }

    // ── Parametric: every level × every provider produces valid JSON ─────────

    #[test]
    fn parametric_every_level_every_provider() {
        let levels = [
            ReasoningLevel::Off,
            ReasoningLevel::Low,
            ReasoningLevel::Medium,
            ReasoningLevel::High,
        ];
        let providers = [
            ProviderKind::OpenAi,
            ProviderKind::DeepSeek,
            ProviderKind::Qwen,
            ProviderKind::Anthropic,
            ProviderKind::GoogleAi,
            ProviderKind::Generic,
        ];

        for level in levels {
            for provider in providers {
                let mut body = json!({"model": "test"});
                ThinkingConfig::new(level).apply_to_body(&mut body, provider);
                assert!(
                    body.is_object(),
                    "body must remain an object for {level:?} × {provider:?}"
                );
                assert_eq!(body["model"], "test", "existing fields preserved");
            }
        }
    }
}
