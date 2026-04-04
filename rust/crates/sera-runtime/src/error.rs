//! Runtime error types.

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum RuntimeError {
    #[error("LLM client error: {0}")]
    Llm(String),

    #[error("Tool execution error: {0}")]
    Tool(String),

    #[error("Context overflow: message count {0} exceeds limit")]
    ContextOverflow(usize),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}
