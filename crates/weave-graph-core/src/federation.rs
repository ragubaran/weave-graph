//! `federation` feature (`plan.md` §2.3, `impl.md` M2.1): composite keys
//! for isolated per-repo subgraph composition, and Tarjan's SCC to isolate
//! circular cross-repo dependencies. Complements — does not replace — the
//! visited-set requirement on every traversal (`plan.md` §1.2a). No
//! network dependency enters this module; composition is purely local.

use std::collections::HashMap;

/// `[repo_id]::[path]::[symbol]` (`plan.md` §2.3) — the composite natural
/// key that keeps two repos' identically-named files/symbols from
/// colliding once composed into one addressable graph.
pub fn composite_key(repo_id: &str, path: &str, symbol: &str) -> String {
    format!("{repo_id}::{path}::{symbol}")
}

/// Every strongly-connected component of `edges` restricted to `node_ids`
/// — a singleton for a node with no cycle through it, a longer group for
/// a real cycle (e.g. a federated A→B→C→A dependency). Group order and
/// order within a group carry no meaning beyond "these ids form one SCC."
pub fn tarjan_scc(node_ids: &[u32], edges: &[(u32, u32)]) -> Vec<Vec<u32>> {
    let mut adjacency: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(u, v) in edges {
        adjacency.entry(u).or_default().push(v);
    }

    let mut state = TarjanState {
        index_counter: 0,
        index: HashMap::new(),
        lowlink: HashMap::new(),
        on_stack: HashMap::new(),
        stack: Vec::new(),
        adjacency,
        result: Vec::new(),
    };
    for &id in node_ids {
        if !state.index.contains_key(&id) {
            state.strong_connect(id);
        }
    }
    state.result
}

struct TarjanState {
    index_counter: u32,
    index: HashMap<u32, u32>,
    lowlink: HashMap<u32, u32>,
    on_stack: HashMap<u32, bool>,
    stack: Vec<u32>,
    adjacency: HashMap<u32, Vec<u32>>,
    result: Vec<Vec<u32>>,
}

impl TarjanState {
    // ponytail: recursive DFS — stack depth tracks the federated composite
    // graph's longest dependency chain, not the 500k-symbol core scale
    // Core Invariant 4 targets. Fine for repo-to-repo linking; revisit
    // with an explicit worklist if that ever changes.
    fn strong_connect(&mut self, v: u32) {
        let v_index = self.index_counter;
        self.index.insert(v, v_index);
        self.lowlink.insert(v, v_index);
        self.index_counter += 1;
        self.stack.push(v);
        self.on_stack.insert(v, true);

        let neighbors = self.adjacency.get(&v).cloned().unwrap_or_default();
        for w in neighbors {
            if !self.index.contains_key(&w) {
                self.strong_connect(w);
                let low_w = self.lowlink.get(&w).copied().unwrap_or(u32::MAX);
                let low_v = self.lowlink.get(&v).copied().unwrap_or(u32::MAX);
                self.lowlink.insert(v, low_v.min(low_w));
            } else if self.on_stack.get(&w).copied().unwrap_or(false) {
                let idx_w = self.index.get(&w).copied().unwrap_or(u32::MAX);
                let low_v = self.lowlink.get(&v).copied().unwrap_or(u32::MAX);
                self.lowlink.insert(v, low_v.min(idx_w));
            }
        }

        let low_v = self.lowlink.get(&v).copied().unwrap_or(u32::MAX);
        let idx_v = self.index.get(&v).copied().unwrap_or(u32::MAX);
        if low_v == idx_v {
            let mut component = Vec::new();
            while let Some(w) = self.stack.pop() {
                self.on_stack.insert(w, false);
                component.push(w);
                if w == v {
                    break;
                }
            }
            self.result.push(component);
        }
    }
}

#[cfg(test)]
mod tests;
