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

use sera_types::memory::SegmentKind;

/// Priority of the persona immutable anchor. `0` is never evicted, matching
/// the Soul convention in [`sera_types::memory::MemoryBlock`].
pub const PRIORITY_IMMUTABLE_ANCHOR: u8 = 0;
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

/// An ordered collection of [`ContextSegment`]s with budget bookkeeping.
#[derive(Debug, Clone)]
pub struct ContextEnvelope {
    segments: Vec<ContextSegment>,
    /// `true` when budget pressure forced at least one eviction.
    pub pressure: bool,
}

impl ContextEnvelope {
    /// Borrow the retained segments in render order.
    pub fn segments(&self) -> &[ContextSegment] {
        &self.segments
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
            pressure = self.pressure,
            "context envelope assembled"
        );
    }

    /// Total characters across retained segment contents.
    pub fn total_chars(&self) -> usize {
        self.segments.iter().map(|s| s.content.chars().count()).sum()
    }
}

/// Builds a [`ContextEnvelope`] by pushing segments in canonical order, then
/// applying an optional char budget.
#[derive(Debug, Default)]
pub struct ContextEnvelopeBuilder {
    segments: Vec<ContextSegment>,
    budget_chars: Option<usize>,
}

impl ContextEnvelopeBuilder {
    /// New builder with an optional char budget. `None` = unlimited =
    /// live behaviour unchanged.
    pub fn new(budget_chars: Option<usize>) -> Self {
        Self {
            segments: Vec::new(),
            budget_chars,
        }
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
        } = self;

        let Some(budget) = budget_chars.filter(|b| *b > 0) else {
            // Unlimited (None / 0): no eviction, no pressure.
            return ContextEnvelope {
                segments,
                pressure: false,
            };
        };

        let total: usize = segments.iter().map(|s| s.content.chars().count()).sum();
        if total <= budget {
            return ContextEnvelope {
                segments,
                pressure: false,
            };
        }

        // Over budget: evict whole segments, highest priority value first,
        // never the immutable anchor. Among equal priorities, evict later
        // (input-order) segments first so earlier context is preferentially
        // retained.
        let mut keep: Vec<bool> = vec![true; segments.len()];
        let mut current = total;
        let mut pressure = false;

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
            if current <= budget {
                break;
            }
            let seg = &segments[idx];
            tracing::warn!(
                kind = ?seg.kind,
                source = %seg.source,
                priority = seg.priority,
                char_len = seg.content.chars().count(),
                budget,
                "context envelope budget pressure: evicting segment"
            );
            keep[idx] = false;
            current -= seg.content.chars().count();
            pressure = true;
        }

        let retained: Vec<ContextSegment> = segments
            .into_iter()
            .zip(keep)
            .filter_map(|(seg, k)| k.then_some(seg))
            .collect();

        ContextEnvelope {
            segments: retained,
            pressure,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(source: &str, content: &str, priority: u8) -> ContextSegment {
        ContextSegment::new(SegmentKind::Custom(source.into()), source, content, priority)
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
    fn total_chars_counts_retained_only() {
        let mut b = ContextEnvelopeBuilder::new(Some(6));
        b.push(seg("persona.immutable_anchor", "anchor", PRIORITY_IMMUTABLE_ANCHOR))
            .push(seg("semantic_recall", "recall", PRIORITY_SEMANTIC_RECALL));
        let env = b.build();
        assert_eq!(env.total_chars(), 6);
    }
}
