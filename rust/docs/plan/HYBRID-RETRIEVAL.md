# Hybrid Retrieval in ContextAssembler

**Feature:** sera-t5k — Hybrid retrieval (index + vector + recency in ContextAssembler)
**Status:** Design Document
**Last Updated:** 2026-04-15

## Problem Statement

Currently, `ContextPipeline` (the `ContextEngine` implementation) assembles context by simply concatenating all messages in memory. This approach:

1. **Misses semantically related content** — Pure vector search may miss topically related but semantically distant blocks
2. **Has no recency awareness** — Recently written/accessed blocks that are highly relevant aren't boosted
3. **No structured index lookup** — The existing `SearchStrategy::Hybrid` enum exists in `sera-types` but is unimplemented

## Design Goals

Enhance the context assembly pipeline with hybrid retrieval combining:

1. **Structured index lookup** — Keyword/heading-based exact matches
2. **Vector similarity** — Embedding-based semantic search
3. **Recency boost** — Recently written/accessed memory blocks weighted higher
4. **Merge and deduplicate** — Combine results from multiple retrieval methods

## Architecture

### Current Structure

```
┌──────────────────┐     ┌─────────────────────┐
│   TurnContext    │────▶│  DefaultRuntime    │
└──────────────────┘     └──────────┬──────────┘
                                     │
                                     ▼
                           ┌─────────────────────┐
                           │  ContextPipeline     │
                           │ (ContextEngine)     │
                           └──────────┬──────────┘
                                     │
                    ┌────────────────┼────────────────┐
                    │                │                │
                    ▼                ▼                ▼
             ┌──────────┐    ┌──────────┐    ┌──────────┐
             │ Messages │    │Condensers│    │ (future) │
             └──────────┘    └──────────┘    └──────────┘
```

### Proposed Structure

```
┌──────────────────┐     ┌─────────────────────┐
│   TurnContext    │────▶│  DefaultRuntime    │
└──────────────────┘     └──────────┬──────────┘
                                     │
                                     ▼
                           ┌─────────────────────┐
                           │  ContextPipeline     │
                           └──────────┬──────────┘
                                     │
                    ┌────────────────┼────────────────┐
                    │                │                │
                    ▼                ▼                ▼
             ┌──────────┐    ┌──────────┐    ┌──────────┐
             │ Messages │    │Condensers│    │ Hybrid   │
             └──────────┘    └──────────┘    │Retrieval │
                                          └──────────┘
                                               │
                    ┌────────────────────────────┼────────────────────────────┐
                    │                │                │                │           │
                    ▼                ▼                ▼                ▼           ▼
             ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐  ┌──────────┐
             │ Keyword  │    │ Vector   │    │ Recency  │    │ Index   │  │ Merge &   │
             │ Search  │    │ Search   │    │ Boost   │    │ Lookup  │  │ Dedupe   │
             └──────────┘    └──────────┘    └──────────┘    └──────────┘  └──────────┘
```

## Implementation Plan

### Phase 1: Add retrieval capability to ContextEngine trait

Extend `ContextEngine` trait with retrieval method:

```rust
#[async_trait]
pub trait ContextEngine: Send + Sync {
    // ... existing methods ...
    
    /// Retrieve relevant context for a query using hybrid search.
    async fn retrieve(
        &self, 
        query: &str,
        strategy: HybridStrategy,
        max_tokens: u32,
    ) -> Result<RetrievedContext, ContextError>;
}
```

### Phase 2: Create HybridRetrieval module

New module: `sera-runtime/src/context_engine/hybrid.rs`

```rust
pub struct HybridRetrieval {
    memory_backend: Box<dyn MemoryBackend>,
    embedding_service: Option<Box<dyn EmbeddingService>>,
    // Configuration
    keyword_weight: f32,
    vector_weight: f32,
    recency_boost_days: u32,
    recency_boost_factor: f32,
}

impl HybridRetrieval {
    pub fn new(memory_backend: Box<dyn MemoryBackend>) -> Self { ... }
    
    pub fn with_embedding_service(mut self, svc: Box<dyn EmbeddingService>) -> Self { ... }
    
    pub async fn retrieve(
        &self, 
        query: &str,
        max_results: u32,
    ) -> Result<Vec<RetrievedMemory>, ContextError> { ... }
    
    async fn keyword_search(&self, query: &str) -> Result<Vec<RankedResult>, ContextError> { ... }
    async fn vector_search(&self, query: &str) -> Result<Vec<RankedResult>, ContextError> { ... }
    fn apply_recency_boost(&self, results: &mut [RankedResult]) { ... }
    fn merge_and_deduplicate(&self, results: Vec<RankedResult>) -> Vec<RankedResult> { ... }
}
```

### Phase 3: Integrate with ContextPipeline

```rust
pub struct ContextPipeline {
    messages: Vec<serde_json::Value>,
    condensers: Vec<Box<dyn Condenser>>,
    // NEW: hybrid retrieval
    retrieval: Option<HybridRetrieval>,
}
```

### Phase 4: Configuration via CapabilityPolicy

Allow per-agent config in capability policy:

```yaml
capability_policies:
  - name: hybrid-retrieval-default
    retrieval:
      enabled: true
      keyword_weight: 0.3
      vector_weight: 0.5
      recency_boost_days: 7
      recency_boost_factor: 1.5
      max_retrieved_tokens: 4000
```

## New Types

### RetrievedMemory

```rust
#[derive(Debug, Clone)]
pub struct RetrievedMemory {
    pub id: MemoryId,
    pub content: String,
    pub source: MemoryTier,
    pub score: f32,
    pub relevance_score: f32,  // 0.0-1.0 from search
    pub recency_score: f32,    // 1.0 for today, decaying
    pub combined_score: f32,  // weighted combination
}
```

### HybridStrategy

```rust
#[derive(Debug, Clone, Default)]
pub struct HybridStrategy {
    pub keyword_weight: f32,    // default 0.3
    pub vector_weight: f32,       // default 0.5  
    pub recency_weight: f32,     // default 0.2
    pub recency_boost_days: u32, // default 7
    pub recency_boost_factor: f32, // default 1.5
    pub fusion_method: FusionMethod,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum FusionMethod {
    /// Weighted sum: score = kw*kws + vec*vecs + rec*recs
    #[default]
    WeightedSum,
    /// Reciprocal rank fusion: RRF = sum(1/(k+r)) for each result
    ReciprocalRank,
}
```

## Embedding Service Trait

Need to add embedding service for vector search:

```rust
#[async_trait]
pub trait EmbeddingService: Send + Sync {
    /// Generate embeddings for text(s).
    async fn embeddings(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, Error>;
    
    /// Compute cosine similarity between two embedding vectors.
    fn similarity(&self, a: &[f32], b: &[f32]) -> f32;
}
```

## Configuration Options

| Option | Default | Description |
|--------|---------|-------------|
| `keyword_weight` | 0.3 | Weight for keyword search results |
| `vector_weight` | 0.5 | Weight for vector similarity results |
| `recency_weight` | 0.2 | Weight for recency boost |
| `recency_boost_days` | 7 | Days to apply recency boost |
| `recency_boost_factor` | 1.5 | Multiplier for recency within window |
| `fusion_method` | WeightedSum | How to combine scores |
| `max_retrieved_tokens` | 4000 | Max tokens from retrieval |

## Design Decisions

1. **Replace vs augment:** Hybrid retrieval is optional, enabled via config. Default pipeline remains backward compatible.

2. **Performance impact:** Each retrieval adds query overhead. Vector search requires embedding service. Consider caching recent query embeddings.

3. **Per-agent config:** Via CapabilityPolicy, allowing different configs per agent.

4. **Fallback:** If vector service unavailable, fall back to keyword-only with warning.

5. **Merge strategy:** Use Reciprocal Rank Fusion (RRF) as default — robust and works well without tuning weights.

## Open Questions

1. Should we use an external embedding service or embed a local model?
2. How to handle retrieval from multiple memory tiers (session/agent/circle)?
3. How to prevent retrieval from polluting context with too much noise?

## Dependencies

- Knowledge index block implementation (depends on: sera-qme)
- Embedding service availability

## Related Issues

- **sera-1u3**: Knowledge explorer UI (depends on memory structure)
- **sera-qme**: Source ingestion workflow (feeds into memory backend)