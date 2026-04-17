//! Language-server registry — maps file extensions to server configurations.
//!
//! Phase 1 ships a single built-in entry for Rust (`rust-analyzer`).
//! See `docs/plan/LSP-TOOLS-DESIGN.md` §5.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for one language server process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    /// Canonical language identifier (LSP `languageId`, e.g. `"rust"`).
    pub language_id: String,
    /// Executable to spawn (looked up on `$PATH` unless absolute).
    pub command: String,
    /// Arguments passed to the executable.
    pub args: Vec<String>,
    /// File extensions (including the leading dot) this server handles.
    pub extensions: Vec<String>,
    /// Opaque JSON passed as `initializationOptions` to the server.
    pub initialization_options: serde_json::Value,
}

impl LspServerConfig {
    /// Default built-in Rust entry (`rust-analyzer`).
    pub fn default_rust() -> Self {
        Self {
            language_id: "rust".to_string(),
            command: "rust-analyzer".to_string(),
            args: Vec::new(),
            extensions: vec![".rs".to_string()],
            initialization_options: serde_json::json!({}),
        }
    }

    /// Default built-in Python entry (`pyright-langserver --stdio`).
    ///
    /// Phase 2 ships this as config-only — CI does not exercise Pyright.
    /// See `docs/plan/LSP-TOOLS-DESIGN.md` §5.
    pub fn default_python() -> Self {
        Self {
            language_id: "python".to_string(),
            command: "pyright-langserver".to_string(),
            args: vec!["--stdio".to_string()],
            extensions: vec![".py".to_string()],
            initialization_options: serde_json::json!({}),
        }
    }

    /// Default built-in TypeScript entry (`typescript-language-server --stdio`).
    ///
    /// Covers both `.ts` and `.tsx`. Config-only in Phase 2.
    pub fn default_typescript() -> Self {
        Self {
            language_id: "typescript".to_string(),
            command: "typescript-language-server".to_string(),
            args: vec!["--stdio".to_string()],
            extensions: vec![".ts".to_string(), ".tsx".to_string()],
            initialization_options: serde_json::json!({}),
        }
    }
}

/// In-memory registry keyed by `language_id`.
#[derive(Debug, Default, Clone)]
pub struct LspServerRegistry {
    by_language: HashMap<String, LspServerConfig>,
}

impl LspServerRegistry {
    /// Empty registry — callers must register their own entries.
    pub fn new() -> Self {
        Self {
            by_language: HashMap::new(),
        }
    }

    /// Registry pre-populated with SERA's shipped defaults (Rust, Python, TypeScript).
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        reg.register(LspServerConfig::default_rust());
        reg.register(LspServerConfig::default_python());
        reg.register(LspServerConfig::default_typescript());
        reg
    }

    /// Register a new language-server configuration. Replaces any prior entry
    /// with the same `language_id`.
    pub fn register(&mut self, config: LspServerConfig) {
        self.by_language.insert(config.language_id.clone(), config);
    }

    /// Resolve a server config by `language_id`.
    pub fn get(&self, language_id: &str) -> Option<&LspServerConfig> {
        self.by_language.get(language_id)
    }

    /// Resolve a server config by file extension (e.g. `".rs"`).
    ///
    /// Extension match is case-sensitive and must include the leading dot to
    /// match how the config is authored.
    pub fn resolve_for_extension(&self, ext: &str) -> Option<&LspServerConfig> {
        self.by_language
            .values()
            .find(|cfg| cfg.extensions.iter().any(|e| e == ext))
    }

    /// Iterate all registered `language_id`s (sorted, for determinism).
    pub fn languages(&self) -> Vec<String> {
        let mut v: Vec<String> = self.by_language.keys().cloned().collect();
        v.sort();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rust_entry_present() {
        let reg = LspServerRegistry::with_defaults();
        let rust = reg.get("rust").expect("rust entry must be present");
        assert_eq!(rust.command, "rust-analyzer");
        assert!(rust.extensions.iter().any(|e| e == ".rs"));
    }

    #[test]
    fn resolve_for_extension_finds_rust() {
        let reg = LspServerRegistry::with_defaults();
        let cfg = reg
            .resolve_for_extension(".rs")
            .expect("rust-analyzer must resolve for .rs");
        assert_eq!(cfg.language_id, "rust");
    }

    #[test]
    fn unknown_extension_returns_none() {
        let reg = LspServerRegistry::with_defaults();
        assert!(reg.resolve_for_extension(".kt").is_none());
        assert!(reg.resolve_for_extension("rs").is_none()); // missing dot
    }

    #[test]
    fn python_default_resolves_for_py() {
        let reg = LspServerRegistry::with_defaults();
        let cfg = reg.resolve_for_extension(".py").expect("python entry");
        assert_eq!(cfg.language_id, "python");
        assert_eq!(cfg.command, "pyright-langserver");
        assert_eq!(cfg.args, vec!["--stdio".to_string()]);
    }

    #[test]
    fn typescript_default_resolves_for_ts_and_tsx() {
        let reg = LspServerRegistry::with_defaults();
        let cfg = reg.resolve_for_extension(".ts").expect("ts entry");
        assert_eq!(cfg.language_id, "typescript");
        let tsx = reg.resolve_for_extension(".tsx").expect("tsx entry");
        assert_eq!(tsx.language_id, "typescript");
    }

    #[test]
    fn register_overwrites_existing() {
        let mut reg = LspServerRegistry::with_defaults();
        reg.register(LspServerConfig {
            language_id: "rust".into(),
            command: "ra-multiplex".into(),
            args: vec!["--proxy".into()],
            extensions: vec![".rs".into()],
            initialization_options: serde_json::json!({}),
        });
        let cfg = reg.get("rust").unwrap();
        assert_eq!(cfg.command, "ra-multiplex");
        assert_eq!(cfg.args, vec!["--proxy".to_string()]);
    }

    #[test]
    fn languages_sorted_for_determinism() {
        let mut reg = LspServerRegistry::new();
        reg.register(LspServerConfig {
            language_id: "rust".into(),
            command: "rust-analyzer".into(),
            args: vec![],
            extensions: vec![".rs".into()],
            initialization_options: serde_json::json!({}),
        });
        reg.register(LspServerConfig {
            language_id: "go".into(),
            command: "gopls".into(),
            args: vec![],
            extensions: vec![".go".into()],
            initialization_options: serde_json::json!({}),
        });
        assert_eq!(reg.languages(), vec!["go".to_string(), "rust".to_string()]);
    }
}
