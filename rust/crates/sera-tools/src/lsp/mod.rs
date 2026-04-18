//! LSP-backed code-introspection tools.
//!
//! Phase 1 scope: `get_symbols_overview` for Rust via rust-analyzer.
//! Phase 2 adds `find_symbol`, `find_referencing_symbols`, and the name-path
//! parser, plus Python / TypeScript registry entries (config-only in CI).
//! See `docs/plan/LSP-TOOLS-DESIGN.md` §13 for the phased rollout.

pub mod cache;
pub mod client;
pub mod error;
pub mod jsonrpc;
pub mod name_path;
pub mod registry;
pub mod state;
pub mod supervisor;
pub mod tools;

pub use cache::{CacheKey, CachedSymbols, Clock, SymbolCache, SystemClock, DEFAULT_TTL};
pub use client::{default_initialize_params, LspClient, LspTransport};
pub use error::{LspError, ToolError};
pub use name_path::{NamePath, NamePathSegment};
pub use registry::{LspServerConfig, LspServerRegistry};
pub use state::{normalize_path, LspToolsState};
pub use supervisor::LspProcessSupervisor;
pub use tools::{
    ByteRange, FindReferencingSymbolsInput, FindReferencingSymbolsResult,
    FindReferencingSymbolsTool, FindSymbolInput, FindSymbolResult, FindSymbolTool,
    GetSymbolsOverviewInput, GetSymbolsOverviewTool, ReferenceMatch, SymbolEntry, SymbolKind,
    SymbolMatch, SymbolsOverview,
};
