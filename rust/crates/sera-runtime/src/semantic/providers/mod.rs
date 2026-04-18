//! Concrete [`sera_types::EmbeddingService`] implementations.
//!
//! Providers that ship today:
//!
//! - [`stub::StubEmbeddingService`] — deterministic hash-derived vectors,
//!   used in unit tests so other crates don't need a live Ollama instance.
//! - [`ollama::OllamaEmbeddingService`] — talks to a local or remote Ollama
//!   daemon via `/api/embeddings`.
//! - [`openai::OpenAIEmbeddingService`] — talks to `api.openai.com/v1/embeddings`.
//! - [`local::LocalEmbeddingService`] (feature `local-embedding`) — pure
//!   on-device CPU embeddings via fastembed-rs + ONNX Runtime, no network
//!   after the first model download. See sera-1t0t.
//!
//! All obey the SPEC-memory §13.3 fail-loudly contract: no silent
//! zero-vector fallbacks, no mock data, every failure surfaces as an
//! [`sera_types::EmbeddingError`].

pub mod ollama;
pub mod openai;
pub mod stub;

#[cfg(feature = "local-embedding")]
pub mod local;
