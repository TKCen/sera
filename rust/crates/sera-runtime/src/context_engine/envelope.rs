//! Ordered context-envelope assembly (sera-ibkr.3).
//!
//! A single ordered assembly path for the system-role context messages the
//! gateway injects ahead of transcript replay. Each injection becomes one
//! [`ContextSegment`]; [`ContextEnvelope::to_messages`] renders them, in
//! order, to one `{"role":"system","content":...}` value apiece — the same
//! shape the gateway's `execute_turn` produced inline. Collapsing the
//! segments into a single message would change live wire behaviour, so the
//! renderer deliberately keeps them separate.
//!
//! ## Budget (sera-ibkr.3)
//!
//! A char-based cap mirrors the spirit of [`sera_types::memory::MemoryBlock`]
//! priority eviction, but kept deliberately simple: whole-segment eviction,
//! no partial truncation. `None` budget (the production default) is unlimited
//! and leaves live behaviour byte-identical. On overflow the lowest-priority
//! (highest `priority` value) evictable segments are dropped first; the
//! persona immutable anchor (`priority == 0`) is **never** evicted. A drop
//! sets the [`ContextEnvelope::pressure`] flag and emits a `warn!` event.
//!
//! An optional **token budget** ([`ContextEnvelopeBuilder::with_budget_tokens`])
//! sits alongside the char budget. When configured it estimates each segment's
//! tokens via [`crate::context_engine::estimate_text_tokens`] (`cl100k_base`,
//! budgeting only) and evicts in the **same** (priority desc, index desc) order
//! until both active budgets are satisfied. The two budgets compose as a union
//! constraint: eviction continues while either cap is exceeded, and each evicted
//! segment is recorded exactly once. A `None`/zero token budget is disabled,
//! leaving char-only behaviour byte-identical (no token estimation performed).

use sera_types::memory::SegmentKind;

use crate::context_engine::estimate_text_tokens;

/// Priority of the persona immutable anchor. `0` is never evicted, matching
/// the Soul convention in [`sera_types::memory::MemoryBlock`].
pub const PRIORITY_IMMUTABLE_ANCHOR: u8 = 0;
/// Priority of the mutable persona / evolving identity — evicted only after
/// all skill/recall segments.
pub const PRIORITY_MUTABLE_PERSONA: u8 = 1;
/// Priority of the evictable base identity memory segments (durable memory,
/// user profile, environment) — more important than skills/recall, evicted
/// before the mutable persona. The soul anchor uses
/// [`PRIORITY_IMMUTABLE_ANCHOR`] instead and is never evicted.
pub const PRIORITY_IDENTITY_MEMORY: u8 = 2;
/// Priority of a trigger-matched skill injection.
pub const PRIORITY_SKILL_INJECTION: u8 = 3;
/// Priority of the skill index block.
pub const PRIORITY_SKILL_INDEX: u8 = 4;
/// Priority of the self-introspection snapshot.
pub const PRIORITY_SELF_INTROSPECTION: u8 = 5;
/// Priority of semantic recall — the most evictable segment.
pub const PRIORITY_SEMANTIC_RECALL: u8 = 6;

/// A single unit of assembled context, rendered to one system message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSegment {
    /// What produced this segment. Reuses [`SegmentKind`]; uses
    /// [`SegmentKind::Custom`] where no dedicated variant fits (skill index,
    /// self-introspection).
    pub kind: SegmentKind,
    /// Provenance label, e.g. `"persona.immutable_anchor"`, `"skill_dispatch"`,
    /// `"skill_index"`, `"self_introspection"`, `"semantic_recall"`.
    pub source: String,
    /// Rendered text injected as the system message content.
    pub content: String,
    /// Lower = more important. `0` is never evicted.
    pub priority: u8,
}

impl ContextSegment {
    /// Construct a segment with an explicit priority.
    pub fn new(
        kind: SegmentKind,
        source: impl Into<String>,
        content: impl Into<String>,
        priority: u8,
    ) -> Self {
        Self {
            kind,
            source: source.into(),
            content: content.into(),
            priority,
        }
    }
}

/// A structured record of a segment dropped during budget eviction.
/// Carries provenance for the pressure audit (sera-ibkr.3) without retaining
/// the (potentially large) segment content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvictedSegment {
    /// What produced the dropped segment.
    pub kind: SegmentKind,
    /// Provenance label of the dropped segment.
    pub source: String,
    /// Priority the segment carried when evicted.
    pub priority: u8,
    /// Character length of the dropped segment content.
    pub char_len: usize,
    /// Estimated token length of the dropped segment content. `Some` only when
    /// a token budget was configured — tokens are estimated only under an active
    /// token budget, since estimation isn't free.
    pub token_len: Option<usize>,
}

/// An ordered collection of [`ContextSegment`]s with budget bookkeeping.
#[derive(Debug, Clone)]
pub struct ContextEnvelope {
    segments: Vec<ContextSegment>,
    /// Segments dropped by budget eviction, in eviction order. Empty when no
    /// pressure occurred.
    evicted: Vec<EvictedSegment>,
    /// `true` when budget pressure forced at least one eviction.
    pub pressure: bool,
    /// Sum of retained segments' estimated tokens, computed during `build`
    /// **only** when a token budget was configured. `None` when the token
    /// budget is disabled (no estimation was performed).
    total_tokens: Option<usize>,
}

impl ContextEnvelope {
    /// Borrow the retained segments in render order.
    pub fn segments(&self) -> &[ContextSegment] {
        &self.segments
    }

    /// Borrow the segments dropped by budget eviction, in eviction order.
    /// Empty when no pressure occurred.
    pub fn evicted(&self) -> &[EvictedSegment] {
        &self.evicted
    }

    /// Render each segment to one `{"role":"system","content":...}` value, in
    /// order. Never collapses segments — one message per segment.
    pub fn to_messages(&self) -> Vec<serde_json::Value> {
        self.segments
            .iter()
            .map(|seg| {
                serde_json::json!({
                    "role": "system",
                    "content": seg.content,
                })
            })
            .collect()
    }

    /// Emit a structured introspection record of every retained segment plus
    /// envelope totals. One `debug!` per segment, one summary line.
    pub fn introspect(&self) {
        for seg in &self.segments {
            tracing::debug!(
                kind = ?seg.kind,
                source = %seg.source,
                priority = seg.priority,
                char_len = seg.content.chars().count(),
                "context envelope segment"
            );
        }
        tracing::debug!(
            segments = self.segments.len(),
            total_chars = self.total_chars(),
            total_tokens = ?self.total_tokens(),
            pressure = self.pressure,
            "context envelope assembled"
        );
    }

    /// Total characters across retained segment contents.
    pub fn total_chars(&self) -> usize {
        self.segments.iter().map(|s| s.content.chars().count()).sum()
    }

    /// Sum of retained segments' estimated tokens, available **only** when a
    /// token budget was configured at build time. `None` when the token budget
    /// is disabled — no estimation was performed.
    pub fn total_tokens(&self) -> Option<usize> {
        self.total_tokens
    }
}

/// Builds a [`ContextEnvelope`] by pushing segments in canonical order, then
/// applying an optional char budget.
#[derive(Debug, Default)]
pub struct ContextEnvelopeBuilder {
    segments: Vec<ContextSegment>,
    budget_chars: Option<usize>,
    /// Optional token cap, composed as a union with the char budget. `None`
    /// (default) = disabled = no token estimation performed.
    budget_tokens: Option<usize>,
}

impl ContextEnvelopeBuilder {
    /// New builder with an optional char budget. `None` = unlimited =
    /// live behaviour unchanged.
    pub fn new(budget_chars: Option<usize>) -> Self {
        Self {
            segments: Vec::new(),
            budget_chars,
            budget_tokens: None,
        }
    }

    /// Configure an optional token budget (sera-ibkr.3). `None`/zero =
    /// disabled = char-only behaviour byte-identical (no token estimation).
    /// Composes with the char budget as a union constraint: eviction continues
    /// while either active cap is exceeded.
    pub fn with_budget_tokens(mut self, budget_tokens: Option<usize>) -> Self {
        self.budget_tokens = budget_tokens;
        self
    }

    /// Append a pre-built segment in canonical order.
    pub fn push(&mut self, segment: ContextSegment) -> &mut Self {
        self.segments.push(segment);
        self
    }

    /// Finalise the envelope, applying budget eviction if a cap is set.
    ///
    /// Eviction drops whole segments by descending priority (most evictable
    /// first), preserving input order among the survivors. `priority == 0`
    /// (the persona immutable anchor) is never dropped, even when the budget
    /// is smaller than the anchor itself.
    pub fn build(self) -> ContextEnvelope {
        let Self {
            segments,
            budget_chars,
            budget_tokens,
        } = self;

        // Effective budgets: zero/None disables that constraint (mirrors the
        // historical char-budget semantics exactly).
        let char_budget = budget_chars.filter(|b| *b > 0);
        let token_budget = budget_tokens.filter(|b| *b > 0);

        // Per-segment char lengths (always cheap) and token estimates (only
        // when a token budget is active — estimation isn't free).
        let char_lens: Vec<usize> = segments.iter().map(|s| s.content.chars().count()).collect();
        let token_lens: Option<Vec<usize>> = token_budget
            .map(|_| segments.iter().map(|s| estimate_text_tokens(&s.content)).collect());

        if char_budget.is_none() && token_budget.is_none() {
            // Both constraints disabled: no eviction, no pressure, and no token
            // total (none was estimated).
            return ContextEnvelope {
                segments,
                evicted: Vec::new(),
                pressure: false,
                total_tokens: None,
            };
        }

        let mut current_chars: usize = char_lens.iter().sum();
        let mut current_tokens: usize = token_lens.as_ref().map_or(0, |t| t.iter().sum());

        // Union constraint: over budget when either active cap is exceeded.
        let over_budget = |chars: usize, tokens: usize| -> bool {
            char_budget.is_some_and(|b| chars > b) || token_budget.is_some_and(|b| tokens > b)
        };

        if !over_budget(current_chars, current_tokens) {
            // Within all active budgets: no eviction. Token total is still
            // reported when a token budget was configured.
            return ContextEnvelope {
                segments,
                evicted: Vec::new(),
                pressure: false,
                total_tokens: token_budget.map(|_| current_tokens),
            };
        }

        // Over budget: evict whole segments, highest priority value first,
        // never the immutable anchor. Among equal priorities, evict later
        // (input-order) segments first so earlier context is preferentially
        // retained.
        let mut keep: Vec<bool> = vec![true; segments.len()];
        let mut pressure = false;
        let mut evicted: Vec<EvictedSegment> = Vec::new();

        // Eviction candidates: indices of non-anchor segments, ordered by
        // (priority desc, index desc) so the most evictable goes first.
        let mut candidates: Vec<usize> = segments
            .iter()
            .enumerate()
            .filter(|(_, s)| s.priority != PRIORITY_IMMUTABLE_ANCHOR)
            .map(|(i, _)| i)
            .collect();
        candidates.sort_by(|&a, &b| {
            segments[b]
                .priority
                .cmp(&segments[a].priority)
                .then(b.cmp(&a))
        });

        for idx in candidates {
            if !over_budget(current_chars, current_tokens) {
                break;
            }
            let seg = &segments[idx];
            let char_len = char_lens[idx];
            let token_len = token_lens.as_ref().map(|t| t[idx]);
            tracing::warn!(
                kind = ?seg.kind,
                source = %seg.source,
                priority = seg.priority,
                char_len,
                budget = ?char_budget,
                token_len = ?token_len,
                budget_tokens = ?token_budget,
                "context envelope budget pressure: evicting segment"
            );
            evicted.push(EvictedSegment {
                kind: seg.kind.clone(),
                source: seg.source.clone(),
                priority: seg.priority,
                char_len,
                token_len,
            });
            keep[idx] = false;
            // saturating_sub for defence in depth: each candidate index is
            // visited at most once today, so plain subtraction never underflows,
            // but this keeps the running totals sound if candidate construction
            // ever changes.
            current_chars = current_chars.saturating_sub(char_len);
            current_tokens = current_tokens.saturating_sub(token_len.unwrap_or(0));
            pressure = true;
        }

        let retained: Vec<ContextSegment> = segments
            .into_iter()
            .zip(keep)
            .filter_map(|(seg, k)| k.then_some(seg))
            .collect();

        ContextEnvelope {
            segments: retained,
            evicted,
            pressure,
            total_tokens: token_budget.map(|_| current_tokens),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(source: &str, content: &str, priority: u8) -> ContextSegment {
        ContextSegment::new(SegmentKind::Custom(source.into()), source, content, priority)
    }

    fn seg_kind(kind: SegmentKind, source: &str, content: &str, priority: u8) -> ContextSegment {
        ContextSegment::new(kind, source, content, priority)
    }

    #[test]
    fn ordering_renders_messages_in_canonical_order() {
        let mut b = ContextEnvelopeBuilder::new(None);
        b.push(seg("persona.immutable_anchor", "anchor", PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("skill_dispatch", "skill", PRIORITY_SKILL_INJECTION))
            .push(seg("semantic_recall", "recall", PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        let msgs = env.to_messages();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["content"], "anchor");
        assert_eq!(msgs[1]["content"], "skill");
        assert_eq!(msgs[2]["content"], "recall");
        for m in &msgs {
            assert_eq!(m["role"], "system");
        }
    }

    #[test]
    fn unlimited_budget_evicts_nothing() {
        let mut b = ContextEnvelopeBuilder::new(None);
        b.push(seg("a", "x".repeat(1000).as_str(), PRIORITY_SKILL_INJECTION))
            .push(seg("semantic_recall", "y".repeat(1000).as_str(), PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert!(!env.pressure);
        assert_eq!(env.segments().len(), 2);
    }

    #[test]
    fn zero_budget_treated_as_unlimited() {
        let mut b = ContextEnvelopeBuilder::new(Some(0));
        b.push(seg("a", "hello", PRIORITY_SKILL_INJECTION));
        let env = b.build();
        assert!(!env.pressure);
        assert_eq!(env.segments().len(), 1);
    }

    #[test]
    fn small_budget_evicts_semantic_recall_first() {
        // anchor(6) + skill_index(10) + recall(10) = 26 chars; budget 20.
        let mut b = ContextEnvelopeBuilder::new(Some(20));
        b.push(seg("persona.immutable_anchor", "anchor", PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("skill_index", "skillindex", PRIORITY_SKILL_INDEX))
            .push(seg("semantic_recall", "recalltext", PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert!(env.pressure);
        // recall (most evictable) dropped; anchor + skill_index retained.
        let sources: Vec<&str> = env.segments().iter().map(|s| s.source.as_str()).collect();
        assert_eq!(sources, vec!["persona.immutable_anchor", "skill_index"]);
    }

    #[test]
    fn eviction_order_recall_then_introspection_then_index() {
        // All evictable segments large; only the anchor must survive.
        let mut b = ContextEnvelopeBuilder::new(Some(6));
        b.push(seg("persona.immutable_anchor", "anchor", PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("skill_dispatch", "skilll", PRIORITY_SKILL_INJECTION))
            .push(seg("skill_index", "indexx", PRIORITY_SKILL_INDEX))
            .push(seg("self_introspection", "introo", PRIORITY_SELF_INTROSPECTION))
            .push(seg("semantic_recall", "recall", PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert!(env.pressure);
        let sources: Vec<&str> = env.segments().iter().map(|s| s.source.as_str()).collect();
        assert_eq!(sources, vec!["persona.immutable_anchor"]);
    }

    #[test]
    fn immutable_anchor_never_evicted_even_when_budget_smaller() {
        // Budget 2, anchor alone is 6 chars — still retained.
        let mut b = ContextEnvelopeBuilder::new(Some(2));
        b.push(seg("persona.immutable_anchor", "anchor", PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("semantic_recall", "recall", PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert!(env.pressure);
        let sources: Vec<&str> = env.segments().iter().map(|s| s.source.as_str()).collect();
        assert_eq!(sources, vec!["persona.immutable_anchor"]);
    }

    #[test]
    fn eviction_records_carry_source_priority_char_len_in_order() {
        // anchor(6) + skill_dispatch(6) + skill_index(6) + recall(6) = 24; budget 6.
        // Only the anchor survives; recall (most evictable) is evicted first,
        // then index, then dispatch.
        let mut b = ContextEnvelopeBuilder::new(Some(6));
        b.push(seg("persona.immutable_anchor", "anchor", PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("skill_dispatch", "skilll", PRIORITY_SKILL_INJECTION))
            .push(seg("skill_index", "indexx", PRIORITY_SKILL_INDEX))
            .push(seg("semantic_recall", "recall", PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert!(env.pressure);
        let evicted = env.evicted();
        let summary: Vec<(&str, u8, usize)> = evicted
            .iter()
            .map(|e| (e.source.as_str(), e.priority, e.char_len))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("semantic_recall", PRIORITY_SEMANTIC_RECALL, 6),
                ("skill_index", PRIORITY_SKILL_INDEX, 6),
                ("skill_dispatch", PRIORITY_SKILL_INJECTION, 6),
            ]
        );
        // The retained anchor must not appear in the eviction record.
        assert!(evicted.iter().all(|e| e.source != "persona.immutable_anchor"));
    }

    #[test]
    fn evicted_empty_when_no_budget() {
        let mut b = ContextEnvelopeBuilder::new(None);
        b.push(seg("a", "x".repeat(1000).as_str(), PRIORITY_SKILL_INJECTION));
        let env = b.build();
        assert!(env.evicted().is_empty());
    }

    #[test]
    fn evicted_empty_when_under_budget() {
        let mut b = ContextEnvelopeBuilder::new(Some(100));
        b.push(seg("persona.immutable_anchor", "anchor", PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("semantic_recall", "recall", PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert!(!env.pressure);
        assert!(env.evicted().is_empty());
    }

    #[test]
    fn identity_memory_evicted_between_skills_and_persona() {
        // soul anchor(6) + mutable persona(6) + identity durable(6) +
        // identity user(6) + skill(6) + recall(6) = 36; budget 24 forces two
        // evictions. Eviction order (priority desc): recall(6) → skill(3) →
        // identity(2,2) → persona(1), so recall + skill are dropped and both
        // identity segments survive alongside soul + persona.
        let mut b = ContextEnvelopeBuilder::new(Some(24));
        b.push(seg_kind(
            SegmentKind::Soul,
            "identity_memory.soul",
            "soulll",
            PRIORITY_IMMUTABLE_ANCHOR,
        ))
        .push(seg("persona.mutable_persona", "person", PRIORITY_MUTABLE_PERSONA))
        .push(seg_kind(
            SegmentKind::Custom("identity_memory.durable_memory".into()),
            "identity_memory.durable_memory",
            "durabl",
            PRIORITY_IDENTITY_MEMORY,
        ))
        .push(seg_kind(
            SegmentKind::Custom("identity_memory.user_profile".into()),
            "identity_memory.user_profile",
            "userpr",
            PRIORITY_IDENTITY_MEMORY,
        ))
        .push(seg("skill_dispatch", "skilll", PRIORITY_SKILL_INJECTION))
        .push(seg("semantic_recall", "recall", PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert!(env.pressure);
        let sources: Vec<&str> = env.segments().iter().map(|s| s.source.as_str()).collect();
        assert_eq!(
            sources,
            vec![
                "identity_memory.soul",
                "persona.mutable_persona",
                "identity_memory.durable_memory",
                "identity_memory.user_profile",
            ]
        );
        // recall(6) and skill(3) are evicted before any identity segment(2),
        // and the budget is satisfied once they are dropped, so both identity
        // segments and the mutable persona(1) are retained.
        let evicted: Vec<(&str, u8, usize)> = env
            .evicted()
            .iter()
            .map(|e| (e.source.as_str(), e.priority, e.char_len))
            .collect();
        assert_eq!(
            evicted,
            vec![
                ("semantic_recall", PRIORITY_SEMANTIC_RECALL, 6),
                ("skill_dispatch", PRIORITY_SKILL_INJECTION, 6),
            ]
        );
    }

    #[test]
    fn identity_memory_evicted_before_mutable_persona() {
        // Tighten budget so an identity segment must go before the mutable
        // persona: soul(6) + persona(6) + identity(6) = 18; budget 12 forces
        // one eviction. The identity segment (priority 2) is more evictable
        // than persona (1), so it is dropped; soul + persona retained.
        let mut b = ContextEnvelopeBuilder::new(Some(12));
        b.push(seg_kind(
            SegmentKind::Soul,
            "identity_memory.soul",
            "soulll",
            PRIORITY_IMMUTABLE_ANCHOR,
        ))
        .push(seg("persona.mutable_persona", "person", PRIORITY_MUTABLE_PERSONA))
        .push(seg_kind(
            SegmentKind::Custom("identity_memory.environment".into()),
            "identity_memory.environment",
            "envvvv",
            PRIORITY_IDENTITY_MEMORY,
        ));
        let env = b.build();
        assert!(env.pressure);
        let sources: Vec<&str> = env.segments().iter().map(|s| s.source.as_str()).collect();
        assert_eq!(sources, vec!["identity_memory.soul", "persona.mutable_persona"]);
        let evicted = env.evicted();
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].source, "identity_memory.environment");
        assert_eq!(evicted[0].priority, PRIORITY_IDENTITY_MEMORY);
        assert_eq!(evicted[0].kind, SegmentKind::Custom("identity_memory.environment".into()));
        assert_eq!(evicted[0].char_len, 6);
    }

    #[test]
    fn total_chars_counts_retained_only() {
        let mut b = ContextEnvelopeBuilder::new(Some(6));
        b.push(seg("persona.immutable_anchor", "anchor", PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("semantic_recall", "recall", PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert_eq!(env.total_chars(), 6);
    }

    // ── Token budget (sera-ibkr.3) ──────────────────────────────────────────

    #[test]
    fn token_only_pressure_evicts_most_evictable() {
        // Chars unlimited; token budget set just below the two-segment sum so
        // the most-evictable (recall) is dropped.
        let anchor = "anchor text";
        let recall = "this is the most evictable recall segment";
        let anchor_t = estimate_text_tokens(anchor);
        // Budget admits only the anchor.
        let budget = anchor_t + estimate_text_tokens(recall) - 1;
        let mut b = ContextEnvelopeBuilder::new(None).with_budget_tokens(Some(budget));
        b.push(seg("persona.immutable_anchor", anchor, PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("semantic_recall", recall, PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert!(env.pressure);
        let sources: Vec<&str> = env.segments().iter().map(|s| s.source.as_str()).collect();
        assert_eq!(sources, vec!["persona.immutable_anchor"]);
        let total = env.total_tokens().expect("token budget active → Some");
        assert!(total <= budget, "retained tokens {total} must be <= {budget}");
    }

    #[test]
    fn token_eviction_order_recall_then_introspection_then_index() {
        // Mirror eviction_order_recall_then_introspection_then_index, driven by
        // a token budget that admits only the anchor.
        let anchor = "anchor";
        let budget = estimate_text_tokens(anchor);
        let mut b = ContextEnvelopeBuilder::new(None).with_budget_tokens(Some(budget));
        b.push(seg("persona.immutable_anchor", anchor, PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("skill_dispatch", "skill dispatch text", PRIORITY_SKILL_INJECTION))
            .push(seg("skill_index", "skill index text", PRIORITY_SKILL_INDEX))
            .push(seg("self_introspection", "self introspection text", PRIORITY_SELF_INTROSPECTION))
            .push(seg("semantic_recall", "semantic recall text", PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert!(env.pressure);
        let sources: Vec<&str> = env.segments().iter().map(|s| s.source.as_str()).collect();
        assert_eq!(sources, vec!["persona.immutable_anchor"]);
        // Evicted in canonical order: recall, introspection, index, dispatch.
        let evicted: Vec<&str> = env.evicted().iter().map(|e| e.source.as_str()).collect();
        assert_eq!(
            evicted,
            vec!["semantic_recall", "self_introspection", "skill_index", "skill_dispatch"]
        );
    }

    #[test]
    fn token_equal_priority_later_evicted_first() {
        // Two equal-priority segments under token pressure: the later one is
        // evicted first so earlier context is preferentially retained.
        let anchor = "anchor";
        let first = "first recall block of words";
        let second = "second recall block of words";
        // Budget admits the anchor plus exactly one recall block.
        let budget = estimate_text_tokens(anchor)
            + estimate_text_tokens(first).max(estimate_text_tokens(second));
        let mut b = ContextEnvelopeBuilder::new(None).with_budget_tokens(Some(budget));
        b.push(seg("persona.immutable_anchor", anchor, PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("recall_first", first, PRIORITY_SEMANTIC_RECALL))
            .push(seg("recall_second", second, PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert!(env.pressure);
        let evicted: Vec<&str> = env.evicted().iter().map(|e| e.source.as_str()).collect();
        assert_eq!(evicted, vec!["recall_second"]);
    }

    #[test]
    fn anchor_never_evicted_under_token_pressure() {
        // Anchor alone exceeds the token budget, yet it is retained.
        let anchor = "this is a fairly long immutable anchor segment that exceeds the budget";
        let budget = 1;
        let mut b = ContextEnvelopeBuilder::new(None).with_budget_tokens(Some(budget));
        b.push(seg("persona.immutable_anchor", anchor, PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("semantic_recall", "recall text", PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert!(env.pressure);
        let sources: Vec<&str> = env.segments().iter().map(|s| s.source.as_str()).collect();
        assert_eq!(sources, vec!["persona.immutable_anchor"]);
    }

    #[test]
    fn char_only_evicted_have_no_token_len_and_total_tokens_none() {
        // Only a char budget set: evicted records carry token_len None and
        // total_tokens() is None.
        let mut b = ContextEnvelopeBuilder::new(Some(6));
        b.push(seg("persona.immutable_anchor", "anchor", PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("semantic_recall", "recall", PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert!(env.pressure);
        assert!(env.total_tokens().is_none());
        assert!(env.evicted().iter().all(|e| e.token_len.is_none()));
    }

    #[test]
    fn combined_char_and_token_pressure_satisfies_union() {
        // Construct so the char budget alone would evict only the largest
        // (recall) segment, but the token budget forces a second eviction.
        let anchor = "anchor";
        let mid = "midmid"; // 6 chars
        let recall = "recallrecallrecall"; // 18 chars, largest
        let anchor_c = anchor.chars().count();
        let mid_c = mid.chars().count();
        let recall_c = recall.chars().count();
        // Char budget admits anchor + mid (evicts only recall).
        let char_budget = anchor_c + mid_c;
        assert!(char_budget < anchor_c + mid_c + recall_c);
        // Token budget admits only the anchor (forces mid out too, beyond the
        // single char-driven recall eviction).
        let token_budget = estimate_text_tokens(anchor);
        let mut b = ContextEnvelopeBuilder::new(Some(char_budget))
            .with_budget_tokens(Some(token_budget));
        b.push(seg("persona.immutable_anchor", anchor, PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("skill_index", mid, PRIORITY_SKILL_INDEX))
            .push(seg("semantic_recall", recall, PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert!(env.pressure);
        // Both recall and mid evicted, each exactly once, in canonical order.
        let evicted: Vec<&str> = env.evicted().iter().map(|e| e.source.as_str()).collect();
        assert_eq!(evicted, vec!["semantic_recall", "skill_index"]);
        // No double-counting: each evicted source unique.
        let mut uniq = evicted.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), evicted.len());
        // Records carry both char_len and Some(token_len).
        assert!(env.evicted().iter().all(|e| e.char_len > 0 && e.token_len.is_some()));
        // Union constraint satisfied where satisfiable: only the anchor remains.
        let sources: Vec<&str> = env.segments().iter().map(|s| s.source.as_str()).collect();
        assert_eq!(sources, vec!["persona.immutable_anchor"]);
        // Retained token total within budget (anchor alone is within token_budget).
        let total = env.total_tokens().expect("token budget active → Some");
        assert!(total <= token_budget);
    }

    #[test]
    fn token_budget_zero_treated_as_disabled() {
        // Some(0) token budget mirrors zero_budget_treated_as_unlimited: no
        // eviction, no token total.
        let mut b = ContextEnvelopeBuilder::new(None).with_budget_tokens(Some(0));
        b.push(seg("a", "hello world this is plenty of text", PRIORITY_SKILL_INJECTION));
        let env = b.build();
        assert!(!env.pressure);
        assert_eq!(env.segments().len(), 1);
        assert!(env.total_tokens().is_none());
    }

    #[test]
    fn evicted_token_len_equals_estimate_of_dropped_content() {
        let anchor = "anchor";
        let recall = "the recall content that will be dropped";
        let budget = estimate_text_tokens(anchor);
        let mut b = ContextEnvelopeBuilder::new(None).with_budget_tokens(Some(budget));
        b.push(seg("persona.immutable_anchor", anchor, PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("semantic_recall", recall, PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert!(env.pressure);
        let evicted = env.evicted();
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].source, "semantic_recall");
        assert_eq!(evicted[0].token_len, Some(estimate_text_tokens(recall)));
    }
}
