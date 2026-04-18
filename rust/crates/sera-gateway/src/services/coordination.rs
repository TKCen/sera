//! Circle coordination policy primitives.
//!
//! Implements SPEC-circles §3 (CoordinationPolicy) and §4 (DAG cycle detection):
//! - Tarjan SCC for cycle detection in agent dependency graphs
//! - `ResultAggregator` trait + `ConcatAggregator` / `FirstWinsAggregator` impls
//! - `ConvergenceConfig` for round-based termination
//! - `ConcurrencyPolicy` enum for serial / parallel / bounded execution

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type NodeId = String;

// -----------------------------------------------------------------------------
// Tarjan SCC cycle detection
// -----------------------------------------------------------------------------

/// Detect strongly-connected components of size > 1 in a directed graph.
///
/// Uses Tarjan's algorithm. Returns each non-trivial SCC as a `Vec<NodeId>`;
/// an empty outer vec means the graph is acyclic (as a DAG).
/// Self-loops are also reported (SCC of size 1 where the node points to itself).
pub fn detect_cycles(edges: &[(NodeId, NodeId)]) -> Vec<Vec<NodeId>> {
    // Build adjacency list and collect unique nodes.
    let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for (from, to) in edges {
        adj.entry(from.clone()).or_default().push(to.clone());
        adj.entry(to.clone()).or_default();
    }

    struct Tarjan<'a> {
        adj: &'a HashMap<NodeId, Vec<NodeId>>,
        index_counter: usize,
        stack: Vec<NodeId>,
        on_stack: HashMap<NodeId, bool>,
        indices: HashMap<NodeId, usize>,
        lowlinks: HashMap<NodeId, usize>,
        result: Vec<Vec<NodeId>>,
    }

    impl<'a> Tarjan<'a> {
        fn strongconnect(&mut self, v: &NodeId) {
            self.indices.insert(v.clone(), self.index_counter);
            self.lowlinks.insert(v.clone(), self.index_counter);
            self.index_counter += 1;
            self.stack.push(v.clone());
            self.on_stack.insert(v.clone(), true);

            if let Some(neighbours) = self.adj.get(v) {
                let neighbours = neighbours.clone();
                for w in &neighbours {
                    if !self.indices.contains_key(w) {
                        self.strongconnect(w);
                        let wl = *self.lowlinks.get(w).unwrap();
                        let vl = *self.lowlinks.get(v).unwrap();
                        self.lowlinks.insert(v.clone(), vl.min(wl));
                    } else if *self.on_stack.get(w).unwrap_or(&false) {
                        let wi = *self.indices.get(w).unwrap();
                        let vl = *self.lowlinks.get(v).unwrap();
                        self.lowlinks.insert(v.clone(), vl.min(wi));
                    }
                }
            }

            // Root of an SCC
            if self.lowlinks.get(v) == self.indices.get(v) {
                let mut component = Vec::new();
                loop {
                    let w = self.stack.pop().expect("stack non-empty");
                    self.on_stack.insert(w.clone(), false);
                    let is_v = &w == v;
                    component.push(w);
                    if is_v {
                        break;
                    }
                }
                // Report non-trivial SCCs (size > 1) or self-loops.
                if component.len() > 1 {
                    self.result.push(component);
                } else if component.len() == 1 {
                    let only = &component[0];
                    if let Some(neighbours) = self.adj.get(only)
                        && neighbours.iter().any(|n| n == only)
                    {
                        self.result.push(component);
                    }
                }
            }
        }
    }

    let mut t = Tarjan {
        adj: &adj,
        index_counter: 0,
        stack: Vec::new(),
        on_stack: HashMap::new(),
        indices: HashMap::new(),
        lowlinks: HashMap::new(),
        result: Vec::new(),
    };

    // Iterate in a stable order for determinism.
    let mut nodes: Vec<NodeId> = adj.keys().cloned().collect();
    nodes.sort();
    for node in &nodes {
        if !t.indices.contains_key(node) {
            t.strongconnect(node);
        }
    }

    t.result
}

// -----------------------------------------------------------------------------
// ResultAggregator
// -----------------------------------------------------------------------------

/// Combines a collection of per-agent results into a single value.
#[async_trait]
pub trait ResultAggregator: Send + Sync {
    async fn aggregate(&self, results: Vec<Value>) -> Result<Value>;
}

/// Joins all results into a JSON array in submission order.
#[derive(Debug, Default, Clone)]
pub struct ConcatAggregator;

#[async_trait]
impl ResultAggregator for ConcatAggregator {
    async fn aggregate(&self, results: Vec<Value>) -> Result<Value> {
        Ok(Value::Array(results))
    }
}

/// Returns the first non-null result, or errors if all results were null/empty.
#[derive(Debug, Default, Clone)]
pub struct FirstWinsAggregator;

#[async_trait]
impl ResultAggregator for FirstWinsAggregator {
    async fn aggregate(&self, results: Vec<Value>) -> Result<Value> {
        results
            .into_iter()
            .find(|v| !v.is_null())
            .ok_or_else(|| anyhow!("FirstWinsAggregator: no non-null results"))
    }
}

// -----------------------------------------------------------------------------
// ConvergenceConfig
// -----------------------------------------------------------------------------

/// Round-based termination config for iterative circle coordination.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ConvergenceConfig {
    pub max_rounds: u32,
    pub min_improvement: f64,
}

impl ConvergenceConfig {
    pub fn new(max_rounds: u32, min_improvement: f64) -> Self {
        Self { max_rounds, min_improvement }
    }

    /// Decide whether to stop after `round` (0-indexed) given the latest
    /// improvement score. Terminates when the round budget is exhausted or
    /// improvement drops below the configured threshold.
    pub fn should_terminate(&self, round: u32, score: f64) -> bool {
        if round + 1 >= self.max_rounds {
            return true;
        }
        score < self.min_improvement
    }
}

// -----------------------------------------------------------------------------
// ConcurrencyPolicy
// -----------------------------------------------------------------------------

/// How agents within a circle are scheduled relative to each other.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ConcurrencyPolicy {
    /// One agent at a time.
    Serial,
    /// Unbounded parallel up to `max` in-flight.
    Parallel { max: usize },
    /// Bounded parallel — same shape as Parallel but semantically a hard cap.
    Bounded { max: usize },
}

impl ConcurrencyPolicy {
    /// Maximum in-flight agent count.
    pub fn capacity(&self) -> usize {
        match self {
            ConcurrencyPolicy::Serial => 1,
            ConcurrencyPolicy::Parallel { max } | ConcurrencyPolicy::Bounded { max } => *max,
        }
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn edge(a: &str, b: &str) -> (NodeId, NodeId) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn test_detect_cycles_acyclic() {
        // a -> b -> c, a -> c (DAG)
        let edges = vec![edge("a", "b"), edge("b", "c"), edge("a", "c")];
        let sccs = detect_cycles(&edges);
        assert!(sccs.is_empty(), "expected no cycles, got {:?}", sccs);
    }

    #[test]
    fn test_detect_cycles_simple_cycle() {
        // a -> b -> c -> a
        let edges = vec![edge("a", "b"), edge("b", "c"), edge("c", "a")];
        let sccs = detect_cycles(&edges);
        assert_eq!(sccs.len(), 1);
        let mut comp = sccs[0].clone();
        comp.sort();
        assert_eq!(comp, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn test_detect_cycles_self_loop() {
        let edges = vec![edge("a", "a")];
        let sccs = detect_cycles(&edges);
        assert_eq!(sccs.len(), 1);
        assert_eq!(sccs[0], vec!["a".to_string()]);
    }

    #[test]
    fn test_detect_cycles_disjoint() {
        // Cycle x<->y, isolated DAG p->q
        let edges = vec![edge("x", "y"), edge("y", "x"), edge("p", "q")];
        let sccs = detect_cycles(&edges);
        assert_eq!(sccs.len(), 1);
        let mut comp = sccs[0].clone();
        comp.sort();
        assert_eq!(comp, vec!["x".to_string(), "y".to_string()]);
    }

    #[tokio::test]
    async fn test_concat_aggregator() {
        let agg = ConcatAggregator;
        let out = agg.aggregate(vec![json!(1), json!("two"), json!({"k": 3})]).await.unwrap();
        assert_eq!(out, json!([1, "two", {"k": 3}]));
    }

    #[tokio::test]
    async fn test_concat_aggregator_empty() {
        let agg = ConcatAggregator;
        let out = agg.aggregate(vec![]).await.unwrap();
        assert_eq!(out, json!([]));
    }

    #[tokio::test]
    async fn test_first_wins_aggregator() {
        let agg = FirstWinsAggregator;
        let out = agg
            .aggregate(vec![Value::Null, json!("winner"), json!("loser")])
            .await
            .unwrap();
        assert_eq!(out, json!("winner"));
    }

    #[tokio::test]
    async fn test_first_wins_all_null_errors() {
        let agg = FirstWinsAggregator;
        let err = agg.aggregate(vec![Value::Null, Value::Null]).await;
        assert!(err.is_err());
    }

    #[test]
    fn test_convergence_terminates_on_max_rounds() {
        let cfg = ConvergenceConfig::new(3, 0.01);
        // round indices 0, 1 should continue (big score), round 2 terminates
        assert!(!cfg.should_terminate(0, 1.0));
        assert!(!cfg.should_terminate(1, 1.0));
        assert!(cfg.should_terminate(2, 1.0));
    }

    #[test]
    fn test_convergence_terminates_on_low_improvement() {
        let cfg = ConvergenceConfig::new(10, 0.05);
        assert!(cfg.should_terminate(0, 0.001));
        assert!(!cfg.should_terminate(0, 0.5));
    }

    #[test]
    fn test_concurrency_capacity() {
        assert_eq!(ConcurrencyPolicy::Serial.capacity(), 1);
        assert_eq!(ConcurrencyPolicy::Parallel { max: 4 }.capacity(), 4);
        assert_eq!(ConcurrencyPolicy::Bounded { max: 8 }.capacity(), 8);
    }
}
