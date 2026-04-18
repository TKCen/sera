//! Tarjan strongly-connected-component detection for task/dependency graphs.
//!
//! Used by Circle coordination (SPEC-circles §4.1) to detect cycles in the
//! member + sub-Circle DAG and promote SCCs to "super-nodes" that execute
//! recursively until their `ConvergenceConfig` terminates them.
//!
//! The algorithm is classic Tarjan (1972) — linear time, iterative
//! (stack-based, not recursive, to avoid stack overflow on deep graphs).

use std::collections::HashMap;
use std::hash::Hash;

/// A single strongly-connected component — a set of nodes where every node
/// is reachable from every other node in the set.
///
/// A component with a single node that has no self-loop is a trivial SCC
/// (not a cycle). Components with `len() > 1` OR with a single node that
/// self-loops are cycles.
pub type Scc<N> = Vec<N>;

/// Compute the strongly-connected components of a directed graph.
///
/// `nodes` lists every node. `edges` is a function returning each node's
/// out-neighbours. The returned SCCs are in reverse topological order
/// (sinks first), per the standard Tarjan output.
///
/// Example:
/// ```
/// use sera_workflow::scc::tarjan_scc;
/// let nodes = vec![1, 2, 3, 4];
/// let edges = |n: &i32| -> Vec<i32> {
///     match n {
///         1 => vec![2],
///         2 => vec![3],
///         3 => vec![1], // cycle: 1 -> 2 -> 3 -> 1
///         4 => vec![],
///         _ => vec![],
///     }
/// };
/// let sccs = tarjan_scc(&nodes, edges);
/// // Two SCCs: {1,2,3} and {4}.
/// assert_eq!(sccs.len(), 2);
/// ```
pub fn tarjan_scc<N, F>(nodes: &[N], mut edges: F) -> Vec<Scc<N>>
where
    N: Eq + Hash + Clone,
    F: FnMut(&N) -> Vec<N>,
{
    let mut state: TarjanState<N> = TarjanState::new();
    for node in nodes {
        if !state.index.contains_key(node) {
            state.strongconnect(node.clone(), &mut edges);
        }
    }
    state.result
}

/// Returns only the cyclic SCCs (components with more than one node OR a
/// single node that self-loops). Trivial single-node SCCs are filtered out.
pub fn cyclic_sccs<N, F>(nodes: &[N], mut edges: F) -> Vec<Scc<N>>
where
    N: Eq + Hash + Clone,
    F: FnMut(&N) -> Vec<N>,
{
    tarjan_scc(nodes, |n| edges(n))
        .into_iter()
        .filter(|scc| {
            if scc.len() > 1 {
                true
            } else if let Some(only) = scc.first() {
                // Self-loop detection.
                edges(only).iter().any(|m| m == only)
            } else {
                false
            }
        })
        .collect()
}

/// True iff the graph contains at least one cycle.
pub fn has_cycle<N, F>(nodes: &[N], edges: F) -> bool
where
    N: Eq + Hash + Clone,
    F: FnMut(&N) -> Vec<N>,
{
    !cyclic_sccs(nodes, edges).is_empty()
}

// --------- internal iterative Tarjan state machine ---------

struct TarjanState<N>
where
    N: Eq + Hash + Clone,
{
    next_index: usize,
    index: HashMap<N, usize>,
    lowlink: HashMap<N, usize>,
    on_stack: HashMap<N, bool>,
    stack: Vec<N>,
    result: Vec<Vec<N>>,
}

impl<N> TarjanState<N>
where
    N: Eq + Hash + Clone,
{
    fn new() -> Self {
        Self {
            next_index: 0,
            index: HashMap::new(),
            lowlink: HashMap::new(),
            on_stack: HashMap::new(),
            stack: Vec::new(),
            result: Vec::new(),
        }
    }

    /// Iterative strongconnect — uses a work stack of (node, successors, i)
    /// frames so deep graphs don't blow the native stack.
    fn strongconnect<F>(&mut self, root: N, edges: &mut F)
    where
        F: FnMut(&N) -> Vec<N>,
    {
        struct Frame<N> {
            node: N,
            succ: Vec<N>,
            i: usize,
        }

        let mut work: Vec<Frame<N>> = Vec::new();

        self.assign_new(root.clone());
        work.push(Frame {
            succ: edges(&root),
            node: root,
            i: 0,
        });

        while let Some(frame) = work.last_mut() {
            if frame.i < frame.succ.len() {
                let w = frame.succ[frame.i].clone();
                frame.i += 1;
                if !self.index.contains_key(&w) {
                    // Recurse on w.
                    self.assign_new(w.clone());
                    let w_succ = edges(&w);
                    work.push(Frame {
                        node: w,
                        succ: w_succ,
                        i: 0,
                    });
                } else if *self.on_stack.get(&w).unwrap_or(&false) {
                    let w_idx = *self.index.get(&w).unwrap();
                    let v_low = *self.lowlink.get(&frame.node).unwrap();
                    self.lowlink.insert(frame.node.clone(), v_low.min(w_idx));
                }
            } else {
                // Finished visiting all successors of frame.node.
                let v = frame.node.clone();
                let v_low = *self.lowlink.get(&v).unwrap();
                let v_idx = *self.index.get(&v).unwrap();

                if v_low == v_idx {
                    // v is the root of an SCC.
                    let mut component: Vec<N> = Vec::new();
                    loop {
                        let w = self.stack.pop().expect("stack non-empty");
                        self.on_stack.insert(w.clone(), false);
                        let is_root = w == v;
                        component.push(w);
                        if is_root {
                            break;
                        }
                    }
                    self.result.push(component);
                }

                work.pop();
                // Propagate lowlink up to the caller frame.
                if let Some(parent) = work.last_mut() {
                    let p_low = *self.lowlink.get(&parent.node).unwrap();
                    self.lowlink.insert(parent.node.clone(), p_low.min(v_low));
                }
            }
        }
    }

    fn assign_new(&mut self, n: N) {
        self.index.insert(n.clone(), self.next_index);
        self.lowlink.insert(n.clone(), self.next_index);
        self.on_stack.insert(n.clone(), true);
        self.stack.push(n);
        self.next_index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edges_of(adj: &HashMap<i32, Vec<i32>>) -> impl FnMut(&i32) -> Vec<i32> + '_ {
        move |n: &i32| adj.get(n).cloned().unwrap_or_default()
    }

    #[test]
    fn empty_graph_has_no_sccs() {
        let nodes: Vec<i32> = vec![];
        let sccs = tarjan_scc(&nodes, |_: &i32| vec![]);
        assert!(sccs.is_empty());
        assert!(!has_cycle(&nodes, |_: &i32| vec![]));
    }

    #[test]
    fn dag_produces_only_trivial_sccs() {
        // 1 -> 2 -> 3, no cycle.
        let mut adj: HashMap<i32, Vec<i32>> = HashMap::new();
        adj.insert(1, vec![2]);
        adj.insert(2, vec![3]);
        adj.insert(3, vec![]);
        let nodes = vec![1, 2, 3];
        let sccs = tarjan_scc(&nodes, edges_of(&adj));
        assert_eq!(sccs.len(), 3);
        assert!(sccs.iter().all(|c| c.len() == 1));
        assert!(!has_cycle(&nodes, edges_of(&adj)));
        assert!(cyclic_sccs(&nodes, edges_of(&adj)).is_empty());
    }

    #[test]
    fn simple_three_cycle() {
        // 1 -> 2 -> 3 -> 1
        let mut adj: HashMap<i32, Vec<i32>> = HashMap::new();
        adj.insert(1, vec![2]);
        adj.insert(2, vec![3]);
        adj.insert(3, vec![1]);
        let nodes = vec![1, 2, 3];
        let sccs = tarjan_scc(&nodes, edges_of(&adj));
        assert_eq!(sccs.len(), 1);
        let mut found = sccs[0].clone();
        found.sort();
        assert_eq!(found, vec![1, 2, 3]);
        assert!(has_cycle(&nodes, edges_of(&adj)));
    }

    #[test]
    fn self_loop_is_a_cycle() {
        let mut adj: HashMap<i32, Vec<i32>> = HashMap::new();
        adj.insert(1, vec![1]);
        let nodes = vec![1];
        assert!(has_cycle(&nodes, edges_of(&adj)));
        let cyc = cyclic_sccs(&nodes, edges_of(&adj));
        assert_eq!(cyc.len(), 1);
        assert_eq!(cyc[0], vec![1]);
    }

    #[test]
    fn disconnected_graph_with_one_cycle_component() {
        // {1<->2} plus isolated 3, 4 DAG (3 -> 4).
        let mut adj: HashMap<i32, Vec<i32>> = HashMap::new();
        adj.insert(1, vec![2]);
        adj.insert(2, vec![1]);
        adj.insert(3, vec![4]);
        adj.insert(4, vec![]);
        let nodes = vec![1, 2, 3, 4];
        let sccs = tarjan_scc(&nodes, edges_of(&adj));
        assert_eq!(sccs.len(), 3);
        let cyc = cyclic_sccs(&nodes, edges_of(&adj));
        assert_eq!(cyc.len(), 1);
        let mut c = cyc[0].clone();
        c.sort();
        assert_eq!(c, vec![1, 2]);
    }

    #[test]
    fn two_overlapping_cycles_form_single_scc() {
        // 1 -> 2 -> 3 -> 1 and 2 -> 4 -> 2  -> single SCC of {1,2,3,4}
        let mut adj: HashMap<i32, Vec<i32>> = HashMap::new();
        adj.insert(1, vec![2]);
        adj.insert(2, vec![3, 4]);
        adj.insert(3, vec![1]);
        adj.insert(4, vec![2]);
        let nodes = vec![1, 2, 3, 4];
        let cyc = cyclic_sccs(&nodes, edges_of(&adj));
        assert_eq!(cyc.len(), 1);
        let mut c = cyc[0].clone();
        c.sort();
        assert_eq!(c, vec![1, 2, 3, 4]);
    }
}
