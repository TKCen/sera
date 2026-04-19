use thiserror::Error;

/// Crate-wide error type for sera-eval.
///
/// Kept small intentionally — the runner and adapters will extend it with
/// additional variants as they land. `#[from]` conversions are wired for
/// the dependencies the stub actually uses (YAML parsing, JSON, SQLite).
#[derive(Debug, Error)]
pub enum EvalError {
    #[error("task definition invalid: {0}")]
    TaskDefInvalid(String),

    #[error("suite not found: {0}")]
    SuiteNotFound(String),

    #[error("yaml: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
