//! Adaptive memory char-budget computation.
//!
//! Replaces the old fixed `DEFAULT_MEMORY_CHAR_BUDGET` constant with a per-turn
//! adaptive formula that scales with remaining context space:
//!
//! ```text
//! remaining = ctx_window_chars - system_prompt_chars - history_chars  (saturating)
//! adaptive  = remaining / 4   (≈ 25 % of remaining space)
//! budget    = min(max_budget, adaptive).clamp(MIN_MEMORY_CHAR_BUDGET, max_budget)
//! ```
//!
//! The caller may supply a fixed override via `MemoryBudgetConfig::fixed_chars`; when
//! set, that value is returned unconditionally (adaptive is the default, fixed wins).

/// Absolute upper bound used when no explicit max_budget is provided.
///
/// Kept as a named constant so existing callers that passed a literal can
/// migrate to `DEFAULT_MAX_MEMORY_CHAR_BUDGET` without a breaking change.
pub const DEFAULT_MAX_MEMORY_CHAR_BUDGET: usize = 32_768;

/// Hard floor: memory injection never receives fewer than this many chars.
///
/// Prevents memory blocks from being silently zeroed out when the context is
/// nearly full. A `tracing::warn` fires when the adaptive value falls below
/// this floor and the clamp is applied.
pub const MIN_MEMORY_CHAR_BUDGET: usize = 512;

/// Configuration for [`compute_memory_char_budget`].
///
/// Construct with `Default::default()` for fully adaptive behaviour, or set
/// `fixed_chars` to bypass the adaptive formula entirely.
#[derive(Debug, Clone)]
pub struct MemoryBudgetConfig {
    /// Hard cap on the returned budget (adaptive result is clamped to this).
    /// Defaults to [`DEFAULT_MAX_MEMORY_CHAR_BUDGET`].
    pub max_budget: usize,

    /// When `Some(n)`, return `n` unconditionally (fixed override).
    /// When `None`, use the adaptive formula (default).
    pub fixed_chars: Option<usize>,
}

impl Default for MemoryBudgetConfig {
    fn default() -> Self {
        Self {
            max_budget: DEFAULT_MAX_MEMORY_CHAR_BUDGET,
            fixed_chars: None,
        }
    }
}

/// Compute the adaptive memory char-budget for the current turn.
///
/// # Arguments
///
/// * `ctx_window_chars` — total context window size expressed in **characters**
///   (a rough proxy: tokens × 4).  Use `model_context_tokens * 4` if only a
///   token count is available.
/// * `system_prompt_chars` — length in chars of the assembled system prompt.
/// * `history_chars` — length in chars of the serialised message history.
/// * `cfg` — budget configuration; see [`MemoryBudgetConfig`].
///
/// # Returns
///
/// Number of chars to allocate to memory injection for this turn.
///
/// # Panics
///
/// Never panics — all arithmetic uses saturating operations.
pub fn compute_memory_char_budget(
    ctx_window_chars: usize,
    system_prompt_chars: usize,
    history_chars: usize,
    cfg: &MemoryBudgetConfig,
) -> usize {
    // Fixed override wins unconditionally.
    if let Some(fixed) = cfg.fixed_chars {
        return fixed;
    }

    let remaining = ctx_window_chars
        .saturating_sub(system_prompt_chars)
        .saturating_sub(history_chars);

    // 25 % of remaining space (integer division).
    let adaptive = remaining / 4;

    // Cap at max_budget.
    let capped = adaptive.min(cfg.max_budget);

    // Enforce minimum floor.
    if capped < MIN_MEMORY_CHAR_BUDGET {
        tracing::warn!(
            adaptive,
            capped,
            min = MIN_MEMORY_CHAR_BUDGET,
            "memory char-budget below minimum floor — clamping to MIN_MEMORY_CHAR_BUDGET"
        );
        return MIN_MEMORY_CHAR_BUDGET;
    }

    capped
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Large context window, empty history → adaptive >> max_budget → returns max_budget.
    #[test]
    fn large_window_empty_history_returns_max_budget() {
        let cfg = MemoryBudgetConfig {
            max_budget: 8_192,
            fixed_chars: None,
        };
        // ctx = 512_000 chars, system = 2_000, history = 0
        // remaining = 510_000, adaptive = 127_500 → capped to 8_192
        let budget = compute_memory_char_budget(512_000, 2_000, 0, &cfg);
        assert_eq!(budget, 8_192);
    }

    /// Context window ~90 % full → adaptive is small, well under max_budget.
    #[test]
    fn nearly_full_context_returns_small_budget() {
        let cfg = MemoryBudgetConfig {
            max_budget: 32_768,
            fixed_chars: None,
        };
        // ctx = 100_000 chars, used = 90_000 → remaining = 10_000, adaptive = 2_500
        let budget = compute_memory_char_budget(100_000, 45_000, 45_000, &cfg);
        assert_eq!(budget, 2_500);
        assert!(budget < cfg.max_budget);
    }

    /// History exceeds context window → remaining saturates to 0 → budget = MIN floor.
    #[test]
    fn history_exceeds_context_returns_clamped_min() {
        let cfg = MemoryBudgetConfig::default();
        // system + history > ctx_window → saturating_sub → 0 → adaptive = 0 → clamp
        let budget = compute_memory_char_budget(10_000, 6_000, 8_000, &cfg);
        assert_eq!(budget, MIN_MEMORY_CHAR_BUDGET);
    }

    /// Fixed override bypasses adaptive formula entirely.
    #[test]
    fn fixed_override_returns_fixed_value() {
        let cfg = MemoryBudgetConfig {
            max_budget: 32_768,
            fixed_chars: Some(4_096),
        };
        // Inputs are irrelevant when fixed_chars is set.
        let budget = compute_memory_char_budget(1_000_000, 0, 0, &cfg);
        assert_eq!(budget, 4_096);
    }

    /// Fixed override = 0 is returned as-is (caller opt-in to zero budget).
    #[test]
    fn fixed_override_zero_is_respected() {
        let cfg = MemoryBudgetConfig {
            max_budget: 32_768,
            fixed_chars: Some(0),
        };
        let budget = compute_memory_char_budget(512_000, 0, 0, &cfg);
        assert_eq!(budget, 0);
    }

    /// Default config sanity: reasonable window and moderate history → sensible budget.
    #[test]
    fn default_config_moderate_usage() {
        let cfg = MemoryBudgetConfig::default();
        // ctx = 512_000 (128k tokens × 4), system = 4_000, history = 100_000
        // remaining = 408_000, adaptive = 102_000, capped to DEFAULT_MAX = 32_768
        let budget = compute_memory_char_budget(512_000, 4_000, 100_000, &cfg);
        assert_eq!(budget, DEFAULT_MAX_MEMORY_CHAR_BUDGET);
    }
}
