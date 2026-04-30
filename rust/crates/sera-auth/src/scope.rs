//! Re-export of [`sera_types::tool::ToolScope`] (sera-u4gj).
//!
//! The enum lives in `sera-types` to preserve the leaf-crate property —
//! adding `sera-types -> sera-auth` would invert the workspace dep
//! graph. This module gives policy code in `sera-auth` (and downstream
//! callers reaching for the auth surface) a stable
//! `sera_auth::scope::ToolScope` import path.
//!
//! No additional behavior lives here yet. Helpers (intersection, narrowing,
//! preset construction) land alongside `PrincipalPolicy` in PR2.

pub use sera_types::tool::ToolScope;

#[cfg(test)]
mod tests {
    use super::ToolScope;

    /// sera-u4gj — the re-export must preserve type identity with the
    /// leaf-crate definition. Without this, downstream crates that import
    /// `sera_auth::ToolScope` and `sera_types::tool::ToolScope` would see
    /// two distinct types.
    #[test]
    fn reexport_is_same_type_as_sera_types_tool_toolscope() {
        let from_auth: ToolScope = ToolScope::Execute;
        let from_types: sera_types::tool::ToolScope =
            sera_types::tool::ToolScope::Execute;
        assert_eq!(from_auth, from_types);
    }
}
