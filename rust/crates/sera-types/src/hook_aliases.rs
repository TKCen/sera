//! Hermes-aligned hook-point name aliases for cross-project legibility.
//!
//! SERA and Hermes fire hooks at the same lifecycle points but use different
//! names. This module documents the mapping so that future alias additions
//! are one-line changes in a single place.
//!
//! Aliases are implemented via `#[serde(alias = "...")]` on the [`HookPoint`]
//! enum variants so that config files and API payloads accept either the SERA
//! canonical name OR the Hermes alias transparently.
//!
//! ## Current aliases
//!
//! | SERA canonical   | Hermes alias     | Hermes internal  |
//! |------------------|------------------|------------------|
//! | `context_memory` | `pre_agent_turn` | `prefetch_all`   |
//!
//! ## Adding a new alias
//!
//! 1. Add `#[serde(alias = "<hermes_name>")]` to the corresponding variant in
//!    [`crate::hook::HookPoint`].
//! 2. Add a row to the table above.
//! 3. Add a parse test in [`crate::hook`]'s test module (or `tests/hooks.rs`).

/// The full alias mapping as a static slice of `(sera_canonical, hermes_alias)` pairs.
/// Useful for documentation generation, validation tooling, and exhaustive test coverage.
pub const HOOK_POINT_ALIASES: &[(&str, &str)] = &[
    // SERA canonical      Hermes alias
    ("context_memory", "pre_agent_turn"),
];
