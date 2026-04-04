//! Docker error types.

#[derive(Debug, thiserror::Error)]
pub enum DockerError {
    #[error("Docker connection failed: {0}")]
    Connection(String),
    #[error("Docker API error: {0}")]
    Api(String),
}
