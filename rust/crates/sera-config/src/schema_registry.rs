//! JSON Schema registry for SERA resource kinds.
//!
//! Schemas are registered by kind string and used to validate manifest `spec`
//! payloads. Uses the `jsonschema` crate for validation.

use std::collections::HashMap;
use serde_json::Value;
use once_cell::sync::Lazy;
use std::sync::Mutex;

use sera_types::config_manifest::ResourceKind;

/// Errors from schema registry operations.
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("no schema registered for kind: {0}")]
    NotFound(String),
    #[error("schema compile error: {0}")]
    Compile(String),
    #[error("validation failed: {0}")]
    Invalid(String),
}

/// Registry mapping resource kind strings to their JSON Schema definitions.
pub struct SchemaRegistry {
    schemas: HashMap<String, Value>,
}

impl SchemaRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            schemas: HashMap::new(),
        }
    }

    /// Register a JSON Schema for a given `ResourceKind`.
    ///
    /// The schema must be a valid JSON Schema object (`Value::Object`).
    pub fn register(&mut self, kind: ResourceKind, schema: Value) -> Result<(), SchemaError> {
        // Attempt to compile to catch errors early.
        jsonschema::validator_for(&schema)
            .map_err(|e| SchemaError::Compile(e.to_string()))?;
        self.schemas.insert(kind.to_string(), schema);
        Ok(())
    }

    /// Retrieve the raw JSON Schema for a kind, if registered.
    pub fn get_schema(&self, kind: &ResourceKind) -> Option<&Value> {
        self.schemas.get(&kind.to_string())
    }

    /// Validate `payload` against the registered schema for `kind`.
    ///
    /// Returns `Ok(())` if valid, or `SchemaError::Invalid` listing the first
    /// validation error.
    pub fn validate(&self, kind: &ResourceKind, payload: &Value) -> Result<(), SchemaError> {
        let schema = self
            .schemas
            .get(&kind.to_string())
            .ok_or_else(|| SchemaError::NotFound(kind.to_string()))?;

        let validator = jsonschema::validator_for(schema)
            .map_err(|e| SchemaError::Compile(e.to_string()))?;

        validator
            .validate(payload)
            .map_err(|e| SchemaError::Invalid(e.to_string()))?;
        Ok(())
    }

    /// List all registered kind strings.
    pub fn list_kinds(&self) -> Vec<&str> {
        self.schemas.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for SchemaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global default registry (starts empty; populated by callers).
static GLOBAL_REGISTRY: Lazy<Mutex<SchemaRegistry>> =
    Lazy::new(|| Mutex::new(SchemaRegistry::new()));

/// Access the global schema registry.
pub fn global_registry() -> &'static Mutex<SchemaRegistry> {
    &GLOBAL_REGISTRY
}
