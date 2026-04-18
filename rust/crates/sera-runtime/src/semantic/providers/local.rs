//! Pure-local CPU embedding provider (sera-1t0t).
//!
//! Runtime choice: **fastembed-rs** (`fastembed` crate). It bundles ONNX
//! Runtime, tokenisation, pooling, and L2 normalisation end-to-end, which
//! means SERA's local-first profile can produce real embeddings without
//! Ollama, OpenAI, or any network call after the first model download.
//! The alternative (`ort` + `tokenizers` + handwritten pipeline) was
//! rejected because fastembed already ships the exact pre/post-processing
//! needed for `sentence-transformers/all-MiniLM-L6-v2` (mean pooling →
//! L2 normalise), and gating fastembed behind a cargo feature keeps its
//! ~80MB ONNX Runtime dependency out of default builds.
//!
//! The entire module compiles only when `--features local-embedding` is
//! set — users on the Ollama / OpenAI path pay zero binary cost.
//!
//! ## Fail-loudly contract
//!
//! Per SPEC-memory §13.3 and the sera-px3w postmortem, this provider
//! NEVER returns silent zero-vectors. Any load / inference failure
//! surfaces as [`EmbeddingError`] so the caller decides whether to
//! retry, queue, or fail the enclosing operation.
//!
//! ## Environment
//!
//! | Var | Default | Purpose |
//! |-----|---------|---------|
//! | `SERA_LOCAL_EMBEDDING_MODEL` | `sentence-transformers/all-MiniLM-L6-v2` | Model id to load. Must be one of fastembed's supported `EmbeddingModel` variants — see [`resolve_model`]. |
//! | `SERA_LOCAL_EMBEDDING_CACHE_DIR` | `~/.cache/sera/models/` | Override the on-disk model cache root. |
//!
//! Model files are downloaded once into the cache and re-used on
//! subsequent process starts (offline-resumable).

#![cfg(feature = "local-embedding")]

use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

use sera_types::{EmbeddingError, EmbeddingHealth, EmbeddingService};

/// Canonical model id for the default all-MiniLM-L6-v2 configuration.
pub const DEFAULT_LOCAL_MODEL_ID: &str = "sentence-transformers/all-MiniLM-L6-v2";
/// Output dimensionality of `all-MiniLM-L6-v2` (matches fastembed's `ModelInfo::dim`).
pub const DEFAULT_LOCAL_DIMENSIONS: usize = 384;
/// Environment variable that overrides the default model id.
pub const ENV_LOCAL_MODEL: &str = "SERA_LOCAL_EMBEDDING_MODEL";
/// Environment variable that overrides the on-disk cache directory.
pub const ENV_LOCAL_CACHE_DIR: &str = "SERA_LOCAL_EMBEDDING_CACHE_DIR";

/// CPU-only local embedding provider backed by fastembed-rs + ONNX Runtime.
///
/// Instances are cheap to clone only via `Arc`, so this struct is not
/// `Clone` — hold it behind an `Arc<dyn EmbeddingService>`. `TextEmbedding`
/// requires `&mut self` for inference; we wrap it in a `Mutex` so the
/// service can be shared across async tasks while keeping the trait method
/// signature (`&self`) honest.
pub struct LocalEmbeddingService {
    model: Mutex<TextEmbedding>,
    model_id: String,
    dims: usize,
}

impl std::fmt::Debug for LocalEmbeddingService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalEmbeddingService")
            .field("model_id", &self.model_id)
            .field("dims", &self.dims)
            .finish_non_exhaustive()
    }
}

impl LocalEmbeddingService {
    /// Construct a service with the default all-MiniLM-L6-v2 model.
    ///
    /// This triggers a model-file download to the SERA cache directory
    /// on first call; subsequent calls reuse the cached artefacts.
    pub fn new() -> Result<Self, EmbeddingError> {
        Self::with_model(DEFAULT_LOCAL_MODEL_ID)
    }

    /// Construct a service for the given model id. The id must match one
    /// of fastembed's supported Hugging Face codes (see [`resolve_model`]).
    pub fn with_model(model_id: impl Into<String>) -> Result<Self, EmbeddingError> {
        let model_id: String = model_id.into();
        let model = resolve_model(&model_id)?;
        let dims = model_dimensions(&model);
        let cache_dir = resolve_cache_dir()?;

        std::fs::create_dir_all(&cache_dir).map_err(|e| {
            EmbeddingError::Provider(format!(
                "create local embedding cache dir {}: {e}",
                cache_dir.display()
            ))
        })?;

        let options = TextInitOptions::new(model.clone())
            .with_cache_dir(cache_dir)
            .with_show_download_progress(false);

        let text_embedding = TextEmbedding::try_new(options).map_err(|e| {
            let msg = e.to_string();
            // Hugging Face / network errors surface here on first download.
            if looks_like_model_missing(&msg) {
                EmbeddingError::ModelNotAvailable(format!("fastembed load {model_id}: {msg}"))
            } else {
                EmbeddingError::Provider(format!("fastembed load {model_id}: {msg}"))
            }
        })?;

        Ok(Self {
            model: Mutex::new(text_embedding),
            model_id,
            dims,
        })
    }

    /// Convenience constructor that reads `SERA_LOCAL_EMBEDDING_MODEL`
    /// from the environment, falling back to the default all-MiniLM-L6-v2.
    pub fn from_env() -> Result<Self, EmbeddingError> {
        let model_id = std::env::var(ENV_LOCAL_MODEL)
            .unwrap_or_else(|_| DEFAULT_LOCAL_MODEL_ID.to_string());
        Self::with_model(model_id)
    }
}

#[async_trait]
impl EmbeddingService for LocalEmbeddingService {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        // fastembed's `embed` is synchronous and CPU-bound; run it on the
        // blocking pool so we don't stall the async executor. We clone the
        // inputs into the closure because `spawn_blocking` requires
        // `'static` data.
        let texts_owned: Vec<String> = texts.to_vec();
        let expected_dims = self.dims;

        // Grab a blocking-safe handle to the inner model. We can't move
        // `&self` into `spawn_blocking`, so we pull the Mutex-guarded
        // object out via a short critical section and perform inference
        // inline. That keeps the lock held during inference, which is
        // fine for a CPU-bound single-threaded ONNX session.
        let raw = {
            let mut guard = self.model.lock().map_err(|e| {
                EmbeddingError::Provider(format!("local embedding mutex poisoned: {e}"))
            })?;
            tokio::task::block_in_place(|| guard.embed(texts_owned, None)).map_err(|e| {
                EmbeddingError::Provider(format!("fastembed inference: {e}"))
            })?
        };

        if raw.len() != texts.len() {
            return Err(EmbeddingError::Provider(format!(
                "fastembed returned {} embeddings for {} inputs",
                raw.len(),
                texts.len()
            )));
        }

        let mut out = Vec::with_capacity(raw.len());
        for vec in raw {
            if vec.len() != expected_dims {
                return Err(EmbeddingError::DimensionMismatch {
                    expected: expected_dims,
                    got: vec.len(),
                });
            }
            out.push(l2_normalise(vec));
        }
        Ok(out)
    }

    async fn health(&self) -> Result<EmbeddingHealth, EmbeddingError> {
        // Cheapest possible probe: run a single-token embedding through
        // the ONNX session. Any ORT / tokenizer failure surfaces loudly.
        let start = std::time::Instant::now();
        let probe = self.embed(&["ok".to_string()]).await?;
        let latency_ms = start.elapsed().as_millis() as u64;
        if probe.len() != 1 || probe[0].len() != self.dims {
            return Ok(EmbeddingHealth {
                available: false,
                detail: format!(
                    "local embedding probe returned {} vectors of len {}",
                    probe.len(),
                    probe.first().map(|v| v.len()).unwrap_or(0)
                ),
                latency_ms: Some(latency_ms),
            });
        }
        Ok(EmbeddingHealth {
            available: true,
            detail: format!("local embedding ok (model={}, dims={})", self.model_id, self.dims),
            latency_ms: Some(latency_ms),
        })
    }
}

/// Map a SERA-facing model id to a fastembed [`EmbeddingModel`].
///
/// We accept both the canonical `sentence-transformers/…` Hugging Face
/// code and fastembed's own enum-variant spelling (`AllMiniLML6V2`) so
/// existing call sites that already serialise the variant name keep
/// working. Unsupported ids return [`EmbeddingError::ModelNotAvailable`].
fn resolve_model(model_id: &str) -> Result<EmbeddingModel, EmbeddingError> {
    match model_id {
        "sentence-transformers/all-MiniLM-L6-v2"
        | "Qdrant/all-MiniLM-L6-v2-onnx"
        | "AllMiniLML6V2"
        | "all-MiniLM-L6-v2" => Ok(EmbeddingModel::AllMiniLML6V2),

        "Xenova/all-MiniLM-L6-v2" | "AllMiniLML6V2Q" | "all-MiniLM-L6-v2-q" => {
            Ok(EmbeddingModel::AllMiniLML6V2Q)
        }

        "sentence-transformers/all-MiniLM-L12-v2" | "AllMiniLML12V2" => {
            Ok(EmbeddingModel::AllMiniLML12V2)
        }

        "BAAI/bge-small-en-v1.5" | "BGESmallENV15" => Ok(EmbeddingModel::BGESmallENV15),
        "BAAI/bge-base-en-v1.5" | "BGEBaseENV15" => Ok(EmbeddingModel::BGEBaseENV15),

        "nomic-ai/nomic-embed-text-v1.5" | "NomicEmbedTextV15" => {
            Ok(EmbeddingModel::NomicEmbedTextV15)
        }

        other => Err(EmbeddingError::ModelNotAvailable(format!(
            "local embedding model {other:?} not supported by fastembed — \
             add a variant to `semantic::providers::local::resolve_model` if needed"
        ))),
    }
}

/// Expected output dimensionality for each supported model.
///
/// Keep in sync with [`resolve_model`]. These values mirror fastembed's
/// `ModelInfo::dim` so we can advertise them before ever calling `embed`.
fn model_dimensions(model: &EmbeddingModel) -> usize {
    match model {
        EmbeddingModel::AllMiniLML6V2
        | EmbeddingModel::AllMiniLML6V2Q
        | EmbeddingModel::AllMiniLML12V2
        | EmbeddingModel::AllMiniLML12V2Q => 384,
        EmbeddingModel::BGESmallENV15 | EmbeddingModel::BGESmallENV15Q => 384,
        EmbeddingModel::BGEBaseENV15 | EmbeddingModel::BGEBaseENV15Q => 768,
        EmbeddingModel::NomicEmbedTextV15 | EmbeddingModel::NomicEmbedTextV15Q => 768,
        // Fallback; callers for these models should pass dims explicitly
        // via a future builder. For now, keep the 384 default for the MVP.
        _ => 384,
    }
}

/// Resolve the on-disk cache directory for ONNX + tokenizer files.
///
/// Precedence:
/// 1. `SERA_LOCAL_EMBEDDING_CACHE_DIR` env var (explicit override).
/// 2. `$HOME/.cache/sera/models/` on Unix / `%LOCALAPPDATA%\sera\models\` on Windows.
/// 3. Current working directory `./sera-models/` as a last resort.
fn resolve_cache_dir() -> Result<PathBuf, EmbeddingError> {
    if let Ok(explicit) = std::env::var(ENV_LOCAL_CACHE_DIR) {
        return Ok(PathBuf::from(explicit));
    }
    if let Some(base) = dirs_cache_dir() {
        return Ok(base.join("sera").join("models"));
    }
    Ok(PathBuf::from("./sera-models"))
}

/// Minimal XDG/LOCALAPPDATA resolver. We avoid pulling in the `dirs`
/// crate here to keep the optional-feature dep surface small; `dirs` is
/// already in the workspace but not pulled into this module to stay
/// self-contained.
fn dirs_cache_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        if let Ok(xdg) = std::env::var("XDG_CACHE_HOME")
            && !xdg.is_empty()
        {
            return Some(PathBuf::from(xdg));
        }
        if let Ok(home) = std::env::var("HOME")
            && !home.is_empty()
        {
            return Some(PathBuf::from(home).join(".cache"));
        }
        None
    }
    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA")
            && !local.is_empty()
        {
            return Some(PathBuf::from(local));
        }
        None
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

/// Heuristic for classifying fastembed load errors. fastembed wraps
/// `hf-hub` errors in an `anyhow::Error`, so we pattern-match on the
/// string form — not ideal, but the crate exposes no richer error type.
fn looks_like_model_missing(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("not found")
        || lower.contains("404")
        || lower.contains("no such")
        || lower.contains("model not supported")
}

/// Defensive L2 normalisation.
///
/// fastembed's default `embed()` path already normalises via the
/// `normalize` helper in its output transformer, but re-normalising
/// costs <1us per 384-dim vector and guarantees the unit-length
/// contract holds even if an upstream change ever flips the default.
fn l2_normalise(mut v: Vec<f32>) -> Vec<f32> {
    let norm_sq: f32 = v.iter().map(|x| x * x).sum();
    let norm = norm_sq.sqrt();
    if norm > f32::EPSILON {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that download a real ONNX model (~80MB) must opt in
    /// explicitly — they are too slow / network-sensitive for CI.
    /// Set `SERA_TEST_LOCAL_EMBEDDING=1` to run them.
    fn model_tests_enabled() -> bool {
        std::env::var("SERA_TEST_LOCAL_EMBEDDING").ok().as_deref() == Some("1")
    }

    #[test]
    fn resolve_model_accepts_canonical_ids() {
        assert!(resolve_model("sentence-transformers/all-MiniLM-L6-v2").is_ok());
        assert!(resolve_model("AllMiniLML6V2").is_ok());
        assert!(resolve_model("all-MiniLM-L6-v2").is_ok());
    }

    #[test]
    fn resolve_model_rejects_unknown() {
        let err = resolve_model("gpt-4-embedding").unwrap_err();
        match err {
            EmbeddingError::ModelNotAvailable(_) => {}
            other => panic!("expected ModelNotAvailable, got {other:?}"),
        }
    }

    #[test]
    fn model_dimensions_all_minilm_l6() {
        assert_eq!(model_dimensions(&EmbeddingModel::AllMiniLML6V2), 384);
    }

    #[test]
    fn l2_normalise_produces_unit_length() {
        let v = vec![3.0, 4.0];
        let n = l2_normalise(v);
        let len: f32 = n.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((len - 1.0).abs() < 1e-6, "expected unit length, got {len}");
    }

    #[test]
    fn l2_normalise_handles_zero_vector() {
        let v = vec![0.0, 0.0, 0.0];
        let n = l2_normalise(v);
        // Zero vector cannot be normalised; must stay finite (all zeros).
        for x in &n {
            assert_eq!(*x, 0.0);
        }
    }

    #[test]
    fn resolve_cache_dir_env_override_wins() {
        let tmp = std::env::temp_dir().join("sera-local-embed-test-cache");
        // SAFETY: this is a test-only mutation; the parent test process
        // is single-threaded w.r.t. env vars within this test function.
        unsafe {
            std::env::set_var(ENV_LOCAL_CACHE_DIR, &tmp);
        }
        let got = resolve_cache_dir().unwrap();
        assert_eq!(got, tmp);
        unsafe {
            std::env::remove_var(ENV_LOCAL_CACHE_DIR);
        }
    }

    // ---- Integration-style tests that actually load the ONNX model ----
    //
    // These are gated behind `SERA_TEST_LOCAL_EMBEDDING=1` because they
    // download ~80MB and take several seconds to initialise. The early
    // `return` pattern keeps them cheap when the env var is unset.

    #[tokio::test]
    async fn local_embedding_embed_single_text() {
        if !model_tests_enabled() {
            return;
        }
        let svc = LocalEmbeddingService::new().expect("construct");
        let v = svc
            .embed(&["hello world".to_string()])
            .await
            .expect("embed");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].len(), 384);
    }

    #[tokio::test]
    async fn local_embedding_embed_batch() {
        if !model_tests_enabled() {
            return;
        }
        let svc = LocalEmbeddingService::new().expect("construct");
        let texts: Vec<String> = (0..32).map(|i| format!("text number {i}")).collect();
        let v = svc.embed(&texts).await.expect("embed batch");
        assert_eq!(v.len(), 32);
        for emb in &v {
            assert_eq!(emb.len(), 384);
        }
    }

    #[tokio::test]
    async fn local_embedding_dimensions_match() {
        if !model_tests_enabled() {
            return;
        }
        let svc = LocalEmbeddingService::new().expect("construct");
        let v = svc.embed(&["dim check".to_string()]).await.expect("embed");
        assert_eq!(svc.dimensions(), v[0].len());
    }

    #[tokio::test]
    async fn local_embedding_deterministic() {
        if !model_tests_enabled() {
            return;
        }
        let svc = LocalEmbeddingService::new().expect("construct");
        let a = svc.embed(&["repeatable".to_string()]).await.expect("a");
        let b = svc.embed(&["repeatable".to_string()]).await.expect("b");
        assert_eq!(a, b, "same input must produce byte-identical output");
    }

    #[tokio::test]
    async fn local_embedding_health_ok() {
        if !model_tests_enabled() {
            return;
        }
        let svc = LocalEmbeddingService::new().expect("construct");
        let h = svc.health().await.expect("health");
        assert!(h.available, "health probe should succeed on ready instance");
    }

    #[tokio::test]
    async fn local_embedding_empty_input_is_no_op() {
        // Safe to run without the env var — empty input short-circuits
        // before any model inference.
        if !model_tests_enabled() {
            return;
        }
        let svc = LocalEmbeddingService::new().expect("construct");
        let v = svc.embed(&[]).await.expect("empty");
        assert!(v.is_empty());
    }
}
