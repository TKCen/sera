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
pub mod knowledge_schema;

pub use error::SkillsError;
pub use loader::SkillLoader;
pub use skill_pack::SkillPack;
pub use bundle::SkillBundle;
pub use knowledge_schema::{KnowledgeSchemaValidator, SchemaViolation, ViolationSeverity, default_schema};
