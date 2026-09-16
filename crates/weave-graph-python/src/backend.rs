//! PyO3 bindings exposing `weave-graph-core`'s query surface —
//! `get_node`, `get_edges`, `query_path`, `impact_radius`,
//! `trace_calls` — to Python, packaged as a separate wheel via maturin.
//! The native `weave` binary never links this crate.
#![deny(unsafe_code)]

use std::sync::Mutex;

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use weave_graph_core::{CsrGraph, Edge, Storage};
use weave_graph_store_sqlite::SqliteStorage;

/// Read-only query handle over one `.weave/graph.db`. The rusqlite
/// connection is `Send` but not `Sync`, so the Mutex is what makes the
/// object safely shareable across Python threads.
#[pyclass]
struct WeaveGraph {
    storage: Mutex<SqliteStorage>,
    csr: CsrGraph,
}

#[pymethods]
impl WeaveGraph {
    /// Open an existing graph database built by `weave index`.
    #[new]
    fn new(path: &str) -> PyResult<Self> {
        let db_path = std::path::Path::new(path);
        if !db_path.exists() {
            return Err(PyValueError::new_err(format!(
                "Database not found at {}",
                path
            )));
        }
        let storage =
            SqliteStorage::open(db_path).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let csr = CsrGraph::load(&storage).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self {
            storage: Mutex::new(storage),
            csr,
        })
    }

    /// One symbol's full record as a dict, or None if `id` is unknown.
    fn get_node<'py>(&self, py: Python<'py>, id: u32) -> PyResult<Option<Bound<'py, PyDict>>> {
        let storage = self
            .storage
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let Some(node) = storage.get_node(id).map_err(py_err)? else {
            return Ok(None);
        };
        let dict = PyDict::new(py);
        dict.set_item("id", node.id)?;
        dict.set_item("repo_id", node.repo_id)?;
        dict.set_item("path", node.path)?;
        dict.set_item("symbol", node.symbol)?;
        dict.set_item("kind", node.kind)?;
        dict.set_item("line_start", node.line_start)?;
        dict.set_item("line_end", node.line_end)?;
        dict.set_item("signature", node.signature)?;
        Ok(Some(dict))
    }

    /// Outbound edges from `id`, each `{source_id, target_id, kind, weight}`.
    fn get_edges<'py>(&self, py: Python<'py>, id: u32) -> PyResult<Vec<Bound<'py, PyDict>>> {
        let storage = self
            .storage
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let edges: Vec<Edge> = storage.get_edges(id).map_err(py_err)?;
        let mut out = Vec::with_capacity(edges.len());
        for e in &edges {
            let dict = PyDict::new(py);
            dict.set_item("source_id", e.source_id)?;
            dict.set_item("target_id", e.target_id)?;
            dict.set_item("kind", e.kind.as_str())?;
            dict.set_item("weight", e.weight)?;
            out.push(dict);
        }
        Ok(out)
    }

    /// Shortest BFS path between two node ids (inclusive of both
    /// endpoints), or None when unreachable.
    fn query_path(&self, from: u32, to: u32) -> PyResult<Option<Vec<u32>>> {
        let storage = self
            .storage
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        storage.query_path(from, to).map_err(py_err)
    }

    /// Every symbol transitively reachable from `symbol` (full BFS over
    /// outbound edges) — the blast-radius count the MCP tool reports,
    /// as a list of `[symbol, path]` pairs.
    fn impact_radius(&self, symbol: &str) -> PyResult<Vec<(String, String)>> {
        let storage = self
            .storage
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let nodes = storage.all_nodes().map_err(py_err)?;
        let Some(root_id) = nodes.iter().find(|n| n.symbol == symbol).map(|n| n.id) else {
            return Err(PyValueError::new_err(format!("symbol not found: {symbol}")));
        };
        let reached = self.csr.reachable_within(root_id, u32::MAX);
        Ok(reached
            .iter()
            .filter_map(|idx| nodes.get(idx as usize))
            .filter(|n| n.id != root_id)
            .map(|n| (n.symbol.clone(), n.path.clone()))
            .collect())
    }

    /// Call-chain trace: outgoing (callees) and incoming (callers) BFS
    /// up to `depth` hops. Returns `(outgoing, incoming)` where each is
    /// a list of `symbol (path:line)` strings, matching the MCP tool's
    /// rendered lines.
    fn trace_calls(&self, symbol: &str, depth: u32) -> PyResult<(Vec<String>, Vec<String>)> {
        let storage = self
            .storage
            .lock()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let nodes = storage.all_nodes().map_err(py_err)?;
        let Some(root_id) = nodes.iter().find(|n| n.symbol == symbol).map(|n| n.id) else {
            return Err(PyValueError::new_err(format!("symbol not found: {symbol}")));
        };
        let outgoing: Vec<String> = self
            .csr
            .reachable_within(root_id, depth)
            .iter()
            .filter_map(|idx| nodes.get(idx as usize))
            .filter(|n| n.id != root_id)
            .map(|n| format!("{} ({}:{})", n.symbol, n.path, n.line_start))
            .collect();
        let incoming = incoming_chain(&*storage, root_id, depth);
        Ok((outgoing, incoming))
    }
}

fn incoming_chain(storage: &dyn Storage, root: u32, depth: u32) -> Vec<String> {
    use std::collections::HashSet;

    let nodes = match storage.all_nodes() {
        Ok(n) => n,
        Err(_) => return Vec::new(),
    };
    let mut visited: HashSet<u32> = HashSet::from([root]);
    let mut queue: std::collections::VecDeque<(u32, u32)> =
        std::collections::VecDeque::from([(root, 0)]);
    let mut result = Vec::new();
    while let Some((current, hop)) = queue.pop_front() {
        if hop >= depth {
            continue;
        }
        let Ok(callers) = storage.get_callers(current) else {
            continue;
        };
        for edge in callers {
            if !visited.insert(edge.source_id) {
                continue;
            }
            if let Some(node) = nodes.iter().find(|n| n.id == edge.source_id) {
                result.push(format!(
                    "{} ({}:{})",
                    node.symbol, node.path, node.line_start
                ));
                queue.push_back((edge.source_id, hop + 1));
            }
        }
    }
    result
}

fn py_err(e: weave_graph_core::StorageError) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// Top-level helpers that don't need a live database connection.
#[pyfunction]
fn schema_version(path: &str) -> PyResult<u32> {
    let storage = SqliteStorage::open(std::path::Path::new(path)).map_err(py_err)?;
    storage.schema_version().map_err(py_err)
}

#[pymodule]
fn weave_graph(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<WeaveGraph>()?;
    m.add_function(wrap_pyfunction!(schema_version, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests;
