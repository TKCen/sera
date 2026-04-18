//! LSP-backed tool implementations.
//!
//! Phase 1 ships `get_symbols_overview`. Phase 2 adds `find_symbol` and
//! `find_referencing_symbols`.

use serde::{Deserialize, Serialize};

pub mod find_referencing_symbols;
pub mod find_symbol;
pub mod get_symbols_overview;
pub use find_referencing_symbols::{
    FindReferencingSymbolsInput, FindReferencingSymbolsResult, FindReferencingSymbolsTool,
    ReferenceMatch,
};
pub use find_symbol::{FindSymbolInput, FindSymbolResult, FindSymbolTool, SymbolMatch};
pub use get_symbols_overview::{GetSymbolsOverviewInput, GetSymbolsOverviewTool};

/// Byte-offset range — stable across editors and CRLF-safe
/// (see `docs/plan/LSP-TOOLS-DESIGN.md` §3.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u32,
    pub end: u32,
}

/// Extensible wrapper around the LSP `SymbolKind` integer.
///
/// Phase 2 replaces the bare `lsp_types::SymbolKind` that phase 1 used for
/// `SymbolEntry::kind`. `lsp_types::SymbolKind` is an opaque `i32` newtype
/// with named constants for 1–26; rust-analyzer occasionally emits
/// language-specific kinds outside that range (see design §14.4).
///
/// We keep this as a transparent `u8` newtype so:
/// * serde round-trips as a plain integer (no struct wrapper on the wire).
/// * unknown values deserialise without failing — future LSP kinds are handled
///   gracefully.
/// * `#[non_exhaustive]` on the struct prevents downstream crates from
///   pattern-matching exhaustively, keeping us free to add associated
///   constants later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct SymbolKind(pub u8);

impl SymbolKind {
    pub const fn new(raw: u8) -> Self {
        Self(raw)
    }

    /// Expose the underlying integer — handy for LSP kind-filter matching.
    pub const fn as_u8(self) -> u8 {
        self.0
    }
}

impl From<lsp_types::SymbolKind> for SymbolKind {
    fn from(kind: lsp_types::SymbolKind) -> Self {
        // `lsp_types::SymbolKind` wraps `i32`; `Debug` prints the integer.
        // Parsing that is the only public way to extract it, so we fall back
        // via the JSON representation.
        //
        // This avoids brittle private-field access while still yielding the
        // correct integer for both the named constants (1..26) and any
        // language-specific extensions.
        match serde_json::to_value(kind) {
            Ok(serde_json::Value::Number(n)) => {
                if let Some(v) = n.as_u64() {
                    Self::from_u64_with_warning(v)
                } else if let Some(v) = n.as_i64() {
                    if v < 0 {
                        tracing::warn!(raw = v, "negative SymbolKind — clamping to 0");
                        Self(0)
                    } else {
                        Self::from_u64_with_warning(v as u64)
                    }
                } else {
                    tracing::warn!(?n, "SymbolKind not representable as integer");
                    Self(0)
                }
            }
            other => {
                tracing::warn!(?other, "SymbolKind did not serialize as number");
                Self(0)
            }
        }
    }
}

impl From<u32> for SymbolKind {
    fn from(raw: u32) -> Self {
        Self::from_u64_with_warning(u64::from(raw))
    }
}

impl SymbolKind {
    fn from_u64_with_warning(raw: u64) -> Self {
        if raw > u8::MAX as u64 {
            tracing::warn!(raw, "SymbolKind out of u8 range — clamping");
            Self(u8::MAX)
        } else {
            Self(raw as u8)
        }
    }
}

/// One entry in the symbol overview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolEntry {
    pub name: String,
    /// Language-server `SymbolKind` integer. See [`SymbolKind`] for the
    /// extensibility story.
    pub kind: SymbolKind,
    pub range: ByteRange,
    #[serde(default)]
    pub children: Vec<SymbolEntry>,
}

/// Result of a `get_symbols_overview` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolsOverview {
    pub path: String,
    pub language: String,
    pub symbols: Vec<SymbolEntry>,
}

#[cfg(test)]
mod symbol_kind_tests {
    use super::*;

    #[test]
    fn serializes_as_plain_integer() {
        let k = SymbolKind::new(23);
        let s = serde_json::to_string(&k).unwrap();
        assert_eq!(s, "23");
    }

    #[test]
    fn deserializes_from_plain_integer() {
        let k: SymbolKind = serde_json::from_str("12").unwrap();
        assert_eq!(k.as_u8(), 12);
    }

    #[test]
    fn unknown_value_deserializes_ok() {
        // 250 is outside the LSP spec but must round-trip.
        let k: SymbolKind = serde_json::from_str("250").unwrap();
        assert_eq!(k.as_u8(), 250);
    }

    #[test]
    fn from_lsp_types_symbol_kind_named_constants() {
        assert_eq!(
            SymbolKind::from(lsp_types::SymbolKind::STRUCT).as_u8(),
            23
        );
        assert_eq!(
            SymbolKind::from(lsp_types::SymbolKind::FUNCTION).as_u8(),
            12
        );
        assert_eq!(
            SymbolKind::from(lsp_types::SymbolKind::INTERFACE).as_u8(),
            11
        );
    }

    #[test]
    fn from_u32_clamps_large_values() {
        assert_eq!(SymbolKind::from(300u32).as_u8(), u8::MAX);
        assert_eq!(SymbolKind::from(23u32).as_u8(), 23);
    }
}
