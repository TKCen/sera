//! LSP-backed code-introspection tools.
//!
//! Phase 1 scope: `get_symbols_overview` for Rust via rust-analyzer.
//! See `docs/plan/LSP-TOOLS-DESIGN.md` §13 for the phased rollout.

pub mod cache;
pub mod client;
pub mod error;
pub mod jsonrpc;
pub mod registry;
pub mod state;
pub mod supervisor;
pub mod tools;

pub use cache::{CacheKey, CachedSymbols, Clock, SymbolCache, SystemClock, DEFAULT_TTL};
pub use client::{default_initialize_params, LspClient, LspTransport};
pub use error::{LspError, ToolError};
pub use registry::{LspServerConfig, LspServerRegistry};
pub use state::{normalize_path, LspToolsState};
pub use supervisor::LspProcessSupervisor;
pub use tools::{
    ByteRange, GetSymbolsOverviewInput, GetSymbolsOverviewTool, SymbolEntry, SymbolsOverview,
};
