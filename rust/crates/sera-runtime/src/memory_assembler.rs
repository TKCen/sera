//! MemoryBlockAssembler — wires the Tier-1 compact memory block into the turn loop.
//!
//! The assembler holds a `MemoryBlock` and renders it each turn, prepending the
//! result to the system/user message sequence before LLM dispatch. When
//! `MemoryBlock::record_turn` returns `true` (overflow_turns >= flush_min_turns),
//! `assemble` signals `did_trigger_pressure = true` so the caller can emit a
//! `memory_pressure` event.

use sera_types::memory::{MemoryBlock, MemorySegment};

/// Result of a single `assemble` call.
#[derive(Debug, Clone)]
pub struct AssembleResult {
    /// Rendered text fragment to prepend to the context (may be empty).
    pub rendered: String,
    /// `true` when `overflow_turns >= flush_min_turns` — caller should emit
    /// a `memory_pressure` event.
    pub did_trigger_pressure: bool,
}

/// Assembles the Tier-1 compact `MemoryBlock` into a prompt fragment each turn.
///
/// # Usage
///
/// ```no_run
/// use sera_runtime::memory_assembler::MemoryBlockAssembler;
/// use sera_types::memory::MemoryBlock;
///
/// let block = MemoryBlock::new(4096);
/// let mut assembler = MemoryBlockAssembler::new(block);
/// let result = assembler.assemble();
/// // Prepend `result.rendered` to the message list before calling `think`.
/// // If `result.did_trigger_pressure` is true, emit a `memory_pressure` event.
/// ```
pub struct MemoryBlockAssembler {
    block: MemoryBlock,
    /// When `false`, `assemble` is a no-op (returns empty string, never triggers
    /// pressure). Controlled by `RuntimeConfig::memory_block_enabled`.
    pub enabled: bool,
}

impl MemoryBlockAssembler {
    /// Create a new assembler wrapping `block`.
    pub fn new(block: MemoryBlock) -> Self {
        Self { block, enabled: true }
    }

    /// Create an assembler that is disabled (no-op).
    pub fn disabled() -> Self {
        Self {
            block: MemoryBlock::new(0),
            enabled: false,
        }
    }

    /// Add a segment to the underlying `MemoryBlock`.
    pub fn push(&mut self, segment: MemorySegment) {
        self.block.push(segment);
    }

    /// Access the underlying `MemoryBlock` (read-only).
    pub fn block(&self) -> &MemoryBlock {
        &self.block
    }

    /// Access the underlying `MemoryBlock` mutably.
    pub fn block_mut(&mut self) -> &mut MemoryBlock {
        &mut self.block
    }

    /// Render the memory block for this turn.
    ///
    /// - Returns empty `rendered` if the block is empty or `enabled` is false.
    /// - Calls `MemoryBlock::record_turn` to advance `overflow_turns`.
    /// - Returns `did_trigger_pressure = true` when `record_turn` returns `true`.
    pub fn assemble(&mut self) -> AssembleResult {
        if !self.enabled || self.block.segments.is_empty() {
            return AssembleResult {
                rendered: String::new(),
                did_trigger_pressure: false,
            };
        }

        let rendered = self.block.render();
        let did_trigger_pressure = self.block.record_turn();

        AssembleResult {
            rendered,
            did_trigger_pressure,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use sera_types::memory::{MemoryBlock, MemorySegment, SegmentKind};

    use super::*;

    fn soul_seg(id: &str, content: &str) -> MemorySegment {
        MemorySegment {
            id: id.to_string(),
            content: content.to_string(),
            priority: 0,
            recency_boost: 1.0,
            char_budget: usize::MAX,
            kind: SegmentKind::Soul,
        }
    }

    fn evictable_seg(id: &str, content: &str, priority: u8) -> MemorySegment {
        MemorySegment {
            id: id.to_string(),
            content: content.to_string(),
            priority,
            recency_boost: 1.0,
            char_budget: usize::MAX,
            kind: SegmentKind::Custom("test".to_string()),
        }
    }

    // ── empty block is a no-op ────────────────────────────────────────────────

    #[test]
    fn empty_block_returns_empty_string_and_no_pressure() {
        let mut asm = MemoryBlockAssembler::new(MemoryBlock::new(4096));
        let result = asm.assemble();
        assert!(result.rendered.is_empty(), "expected empty rendered for empty block");
        assert!(!result.did_trigger_pressure, "empty block must never trigger pressure");
    }

    // ── disabled assembler is a no-op ─────────────────────────────────────────

    #[test]
    fn disabled_assembler_is_noop() {
        let mut asm = MemoryBlockAssembler::disabled();
        // Even with segments, disabled must not render or trigger pressure.
        asm.push(soul_seg("soul", "You are SERA."));
        let result = asm.assemble();
        assert!(result.rendered.is_empty(), "disabled assembler must return empty");
        assert!(!result.did_trigger_pressure, "disabled assembler must never trigger pressure");
    }

    // ── render returns priority-ordered content ───────────────────────────────

    #[test]
    fn assemble_renders_priority_ordered_content() {
        let mut asm = MemoryBlockAssembler::new(MemoryBlock::new(10_000));
        asm.push(evictable_seg("low", "Low priority.", 10));
        asm.push(soul_seg("soul", "Soul content."));
        asm.push(evictable_seg("high", "High priority.", 1));

        let result = asm.assemble();
        // Soul should appear before evictable segments.
        let soul_pos = result.rendered.find("Soul content.").expect("soul must be present");
        let high_pos = result.rendered.find("High priority.").expect("high must be present");
        let low_pos = result.rendered.find("Low priority.").expect("low must be present");
        assert!(soul_pos < high_pos, "Soul must appear before high-priority segment");
        assert!(high_pos < low_pos, "High-priority must appear before low-priority");
    }

    // ── overflow N < flush_min_turns does NOT trigger pressure ────────────────

    #[test]
    fn overflow_below_flush_min_turns_does_not_trigger() {
        // Budget=5, flush_min_turns=3. Soul content is long, always over budget.
        let mut block = MemoryBlock::with_flush_min_turns(5, 3);
        block.push(soul_seg("soul", "This content is much longer than 5 chars."));
        let mut asm = MemoryBlockAssembler::new(block);

        let r1 = asm.assemble(); // overflow_turns → 1
        let r2 = asm.assemble(); // overflow_turns → 2
        assert!(!r1.did_trigger_pressure, "turn 1 must not trigger pressure");
        assert!(!r2.did_trigger_pressure, "turn 2 must not trigger pressure");
    }

    // ── overflow for flush_min_turns consecutive turns DOES trigger pressure ──

    #[test]
    fn overflow_at_flush_min_turns_triggers_pressure() {
        // Budget=5, flush_min_turns=3. Soul content always over budget.
        let mut block = MemoryBlock::with_flush_min_turns(5, 3);
        block.push(soul_seg("soul", "This content is much longer than 5 chars."));
        let mut asm = MemoryBlockAssembler::new(block);

        let r1 = asm.assemble(); // overflow_turns = 1
        let r2 = asm.assemble(); // overflow_turns = 2
        let r3 = asm.assemble(); // overflow_turns = 3 == flush_min_turns → true

        assert!(!r1.did_trigger_pressure);
        assert!(!r2.did_trigger_pressure);
        assert!(r3.did_trigger_pressure, "must trigger at flush_min_turns");
    }

    // ── pressure counter resets when block is back under budget ──────────────

    #[test]
    fn pressure_counter_resets_when_under_budget() {
        // Budget=5, flush_min_turns=3. Start with over-budget soul content.
        let mut block = MemoryBlock::with_flush_min_turns(5, 3);
        block.push(soul_seg("soul", "Too long for budget."));
        let mut asm = MemoryBlockAssembler::new(block);

        asm.assemble(); // overflow_turns = 1
        asm.assemble(); // overflow_turns = 2
        assert_eq!(asm.block().overflow_turns, 2);

        // Replace the over-budget soul segment with a short one that fits in budget=5.
        asm.block_mut().segments.clear();
        asm.block_mut().push(evictable_seg("tiny", "Hi.", 1));

        // Now block has a segment and is under budget → record_turn resets overflow_turns.
        let r = asm.assemble();
        assert!(!r.did_trigger_pressure, "must not trigger after reset");
        assert_eq!(asm.block().overflow_turns, 0, "overflow_turns must be 0 after under-budget turn");
    }
}
