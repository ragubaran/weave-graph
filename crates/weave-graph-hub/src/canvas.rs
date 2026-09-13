//! Registry-side canvas aggregation (feature `hub-canvas`):
//! `GET {prefix}/repos/{repo_id}/canvas` renders the latest committed
//! snapshot's architectural modules as a [JSON Canvas](https://jsoncanvas.org)
//! document — the LOD 1 tier `weave report` already produces locally, now
//! available to anyone with network access to the registry, without
//! needing a checkout.
//!
//! **v1 scope, stated rather than silently narrowed**: LOD 1 (modules)
//! only — no LOD 0 repo root, no LOD 2 per-module file canvases, no LOD 3
//! symbol export. Those need either a registry-side multi-canvas link
//! structure or a second endpoint; this one answers "what does this repo's
//! architecture look like" in a single call, which is what an Obsidian
//! vault sync actually needs per repo. An N-repo view is
//! [`build_mesh_canvas`] below — bands, not a merged cross-repo graph: the
//! registry has no cross-repo edges to draw (`weave link`'s federation
//! data is client-side only, never pushed to the hub), so a mesh call
//! shows each repo's own architecture side by side, not how they connect.
//!
//! **Why this doesn't call `weave-graph-cli::report`**: that crate is a
//! binary (`weave`), not a library — nothing outside it can link against
//! `report::generate`. Rather than restructure a tested, working CLI crate
//! to grow a `[lib]` target for one caller, this module builds its own
//! minimal JSON Canvas shape directly from the same shared clustering
//! primitives (`weave_graph_core::modules`) `report.rs` itself calls —
//! same algorithm, independently rendered, the same boundary `weave
//! blast`'s own module-folding already draws for the same reason.

use std::collections::HashMap;

use serde::Serialize;
use weave_graph_core::modules::{Module, aggregate_file_edges, build_modules};
use weave_graph_core::{NodeId, Storage};
use weave_graph_store_sqlite::SqliteStorage;

/// Mirrors `weave report`'s own budget — a registry serving thousands of
/// modules over HTTP needs the same bound a local
/// `weave report` run does, for the same reason (a canvas viewer, not an
/// unbounded dump). Overflow beyond this is dropped with a count noted in
/// the response, never silently — narrower than `weave report`'s own
/// overflow *sub-canvas* link (a real, separate follow-on, not built here).
const MODULE_BUDGET: usize = 200;

#[derive(Debug, Clone, Serialize)]
pub struct CanvasNode {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Canvas {
    pub nodes: Vec<CanvasNode>,
    /// Modules Louvain found beyond `MODULE_BUDGET`, dropped from `nodes`
    /// — surfaced as a count so a caller can tell truncation from "this
    /// repo genuinely has few modules," never silently.
    pub overflow_count: usize,
}

/// One grid cell per module — the same simple, deterministic layout
/// `weave report`'s own `modules_canvas` uses, so a module's position is
/// stable across regenerations (unlike a force-directed layout, which
/// isn't reproducible without carrying the whole graph's prior state).
fn grid_layout(nodes: &mut [CanvasNode]) {
    const COLS: i32 = 6;
    const CELL_W: i32 = 260;
    const CELL_H: i32 = 140;
    for (i, node) in nodes.iter_mut().enumerate() {
        let i = i as i32;
        node.x = (i % COLS) * CELL_W;
        node.y = (i / COLS) * CELL_H;
    }
}

fn module_node(module: &Module) -> CanvasNode {
    CanvasNode {
        id: format!("module-{}", module.id),
        kind: "text",
        x: 0,
        y: 0,
        width: 240,
        height: 120,
        text: format!("# {}\n\n{} file(s)", module.label, module.files.len()),
    }
}

/// Builds the module-level canvas directly from an already-opened
/// snapshot's nodes/edges — split from [`from_snapshot_bytes`] so it's
/// testable against an in-process `Storage` without round-tripping
/// through a temp file.
pub fn build_module_canvas(
    storage: &dyn Storage,
) -> Result<Canvas, weave_graph_core::StorageError> {
    let nodes = storage.all_nodes()?;
    let edges = storage.all_edges()?;
    let file_of: HashMap<NodeId, String> = nodes.iter().map(|n| (n.id, n.path.clone())).collect();
    let file_edges = aggregate_file_edges(&edges, &file_of);
    let mut modules = build_modules(&nodes, &file_edges);
    modules.sort_by(|a, b| a.label.cmp(&b.label));

    let overflow_count = modules.len().saturating_sub(MODULE_BUDGET);
    modules.truncate(MODULE_BUDGET);

    let mut canvas_nodes: Vec<CanvasNode> = modules.iter().map(module_node).collect();
    grid_layout(&mut canvas_nodes);

    Ok(Canvas {
        nodes: canvas_nodes,
        overflow_count,
    })
}

/// Writes `bytes` (a pulled snapshot — a real `graph.db`, never actually
/// compressed today, see `client.rs`'s own v1-scope note) to a scratch
/// file, opens it read-only, renders the canvas, then always removes the
/// scratch file — even on error, since a registry serving many requests
/// must not accumulate one temp file per canvas fetch.
pub fn from_snapshot_bytes(bytes: &[u8]) -> Result<Canvas, String> {
    let scratch = std::env::temp_dir().join(format!(
        "weave-hub-canvas-{}-{}.db",
        std::process::id(),
        scratch_nonce()
    ));
    let result = (|| {
        std::fs::write(&scratch, bytes).map_err(|e| e.to_string())?;
        let storage = SqliteStorage::open_read_only(&scratch).map_err(|e| e.to_string())?;
        build_module_canvas(&storage).map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_file(&scratch);
    result
}

/// Horizontal band width for [`build_mesh_canvas`] — wide enough that
/// `grid_layout`'s own 6-column module grid (`6 * 260 = 1560`) never spills
/// into the next repo's band, plus a gutter.
const MESH_BAND_WIDTH: i32 = 6 * 260 + 200;

/// Stitches N single-repo module canvases (each already built by
/// [`build_module_canvas`]/`Registry::canvas`) into one mesh view: one
/// horizontal band per repo, a text header node naming it, its own module
/// grid shifted into that band.
///
/// **v1 scope, stated rather than silently narrowed**: no cross-repo edges
/// — the registry only ever stores flat single-repo snapshots (`weave
/// link`'s pairwise federation data lives client-side, never pushed to the
/// hub), so there is no real N-way dependency graph to draw here. This
/// answers "what does each of these repos' architecture look like, side
/// by side in one canvas", not "how do they call each other" — the latter
/// needs federation data the registry doesn't have, a separate,
/// undesigned piece of work.
pub fn build_mesh_canvas(bands: Vec<(String, Canvas)>) -> Canvas {
    let mut nodes = Vec::new();
    let mut overflow_count = 0;
    for (i, (repo_id, band)) in bands.into_iter().enumerate() {
        let x_offset = i as i32 * MESH_BAND_WIDTH;
        nodes.push(CanvasNode {
            id: format!("mesh-header-{repo_id}"),
            kind: "text",
            x: x_offset,
            y: -160,
            width: 240,
            height: 120,
            text: format!("# {repo_id}"),
        });
        for mut module_node in band.nodes {
            module_node.x += x_offset;
            nodes.push(module_node);
        }
        overflow_count += band.overflow_count;
    }
    Canvas {
        nodes,
        overflow_count,
    }
}

/// A per-call scratch-filename disambiguator — this crate's own build
/// already avoids `Instant`/`SystemTime`-based ids elsewhere for
/// determinism reasons that don't apply here (a filesystem temp name has
/// no correctness requirement beyond "don't collide with a concurrent
/// call on the same process"), so wall-clock nanoseconds are fine.
fn scratch_nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
