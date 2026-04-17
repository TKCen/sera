//! `sera-skills` — Skill pack loading and capability discovery for SERA agents.
//!
//! Provides the [`SkillPack`] trait and [`SkillLoader`] for discovering and loading
//! skill packs that describe agent capabilities.
//!
//! # Overview
//!
//! - [`SkillPack`] — trait for skill pack implementations
//! - [`SkillLoader`] — discovers and loads skill packs from various sources
//! - [`SkillBundle`] — a loaded collection of skills with metadata
//!
//! # Example
//!
//! ```rust,ignore
//! use sera_skills::{SkillLoader, FileSystemLoader};
//!
//! let loader = SkillLoader::new(FileSystemLoader::new("./skills".into()));
//! let bundle = loader.load_pack("coding").await?;
//! ```

pub mod error;
pub mod loader;
pub mod skill_pack;
pub mod bundle;
pub mod markdown;
pub mod markdown_pack;
pub mod knowledge_schema;
pub mod knowledge_activity_log;
pub mod knowledge_lint;

pub use error::SkillsError;
pub use loader::{SkillLoader, FileSystemSkillPack};
pub use skill_pack::SkillPack;
pub use bundle::SkillBundle;
pub use markdown::{parse_skill_markdown_file, parse_skill_markdown_str, ParsedSkillMarkdown};
pub use markdown_pack::MarkdownSkillPack;
pub use knowledge_schema::{KnowledgeSchemaValidator, SchemaViolation, ViolationSeverity, default_schema};
pub use knowledge_activity_log::{
    KnowledgeOp,
    KnowledgeActivityEntry,
    KnowledgeActivityLog,
    ActivityLogFilter,
    DEFAULT_MAX_ENTRIES,
};
pub use knowledge_lint::{
    BasicLinter, FindingSeverity, KnowledgeLinter, LintCheckKind, LintConfig, LintError,
    LintFinding, LintReport, PageInfo,
};
