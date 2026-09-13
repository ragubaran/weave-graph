use weave_graph_core::{Edge, Node, Storage};
use weave_graph_store_sqlite::SqliteStorage;

use super::{Canvas, CanvasNode, build_mesh_canvas, build_module_canvas, from_snapshot_bytes};

fn node(id: u32, path: &str) -> Node {
    Node {
        id,
        repo_id: "r".into(),
        path: path.into(),
        symbol: format!("s{id}"),
        kind: "function".into(),
        line_start: 1,
        line_end: 2,
        signature: String::new(),
    }
}

fn edge(source_id: u32, target_id: u32) -> Edge {
    Edge {
        id: 0,
        source_id,
        target_id,
        kind: "CALLS_EXACT".into(),
        weight: 1.0,
    }
}

/// Two densely cross-linked files plus one isolated file — the same
/// two-communities-plus-a-loner shape `weave-graph-core::modules`'s own
/// tests use to prove Louvain is doing real modularity work, not just
/// connected components.
fn fixture() -> SqliteStorage {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage.upsert_node(&node(1, "a.rs")).unwrap();
    storage.upsert_node(&node(2, "a.rs")).unwrap();
    storage.upsert_node(&node(3, "b.rs")).unwrap();
    storage.upsert_node(&node(4, "lonely.rs")).unwrap();
    storage.upsert_edge(&edge(1, 3)).unwrap();
    storage.upsert_edge(&edge(3, 1)).unwrap();
    storage
}

#[test]
fn build_module_canvas_covers_every_file_with_a_node() {
    let storage = fixture();
    let canvas = build_module_canvas(&storage, &[]).unwrap();

    let total_files: usize = canvas
        .nodes
        .iter()
        .map(|n| {
            // Each node's `text` embeds "<n> file(s)" — parsed back out
            // rather than re-deriving module membership, since the point
            // is to check the *rendered* canvas covers every file.
            n.text
                .lines()
                .find_map(|l| l.split_whitespace().next()?.parse::<usize>().ok())
                .unwrap_or(0)
        })
        .sum();
    assert_eq!(total_files, 3, "a.rs (x2 symbols, 1 file), b.rs, lonely.rs");
}

#[test]
fn build_module_canvas_lays_nodes_out_on_a_grid_without_overlap() {
    let storage = fixture();
    let canvas = build_module_canvas(&storage, &[]).unwrap();
    let mut positions: Vec<(i32, i32)> = canvas.nodes.iter().map(|n| (n.x, n.y)).collect();
    positions.sort();
    positions.dedup();
    assert_eq!(
        positions.len(),
        canvas.nodes.len(),
        "every module must get a distinct grid cell"
    );
}

#[test]
fn build_module_canvas_reports_zero_overflow_under_budget() {
    let storage = fixture();
    let canvas = build_module_canvas(&storage, &[]).unwrap();
    assert_eq!(canvas.overflow_count, 0);
}

#[test]
fn build_module_canvas_excludes_matching_modules() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage.upsert_node(&node(1, "internal.rs")).unwrap();
    storage.upsert_node(&node(2, "public.rs")).unwrap();
    // Root-level files share the `(root)` module label, so the exclusion
    // must also match member paths.
    let canvas_exact = build_module_canvas(&storage, &["internal".to_string()]).unwrap();
    assert_eq!(canvas_exact.nodes.len(), 1);
    assert!(canvas_exact.nodes[0].text.contains("1 file(s)"));

    let canvas_prefix = build_module_canvas(&storage, &["int".to_string()]).unwrap();
    assert_eq!(canvas_prefix.nodes.len(), 1);
    assert!(canvas_prefix.nodes[0].text.contains("1 file(s)"));
}

#[test]
fn from_snapshot_bytes_round_trips_a_real_sqlite_file() {
    // Build a real on-disk snapshot the same way a pushed `graph.db`
    // would arrive, then feed its raw bytes through the same path the
    // registry's `/canvas` handler uses.
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("graph.db");
    {
        let mut storage = SqliteStorage::open(&db_path).unwrap();
        storage.upsert_node(&node(1, "a.rs")).unwrap();
        storage.upsert_node(&node(2, "b.rs")).unwrap();
        storage.upsert_edge(&edge(1, 2)).unwrap();
    }
    let bytes = std::fs::read(&db_path).unwrap();

    let canvas = from_snapshot_bytes(&bytes, &[]).unwrap();
    assert!(!canvas.nodes.is_empty());
}

#[test]
fn from_snapshot_bytes_reports_malformed_input_as_an_error_not_a_panic() {
    let err = from_snapshot_bytes(b"not a sqlite file", &[]).unwrap_err();
    assert!(!err.is_empty());
}

fn single_node_canvas(text: &str) -> Canvas {
    Canvas {
        nodes: vec![CanvasNode {
            id: "m".to_string(),
            kind: "text",
            x: 0,
            y: 0,
            width: 240,
            height: 120,
            text: text.to_string(),
        }],
        overflow_count: 0,
    }
}

#[test]
fn build_mesh_canvas_adds_one_header_node_per_repo() {
    let mesh = build_mesh_canvas(vec![
        ("repo-a".to_string(), single_node_canvas("a-module")),
        ("repo-b".to_string(), single_node_canvas("b-module")),
    ]);
    let headers: Vec<&str> = mesh
        .nodes
        .iter()
        .filter(|n| n.id.starts_with("mesh-header-"))
        .map(|n| n.text.as_str())
        .collect();
    assert_eq!(headers, vec!["# repo-a", "# repo-b"]);
}

#[test]
fn build_mesh_canvas_shifts_each_band_so_repos_never_overlap() {
    let mesh = build_mesh_canvas(vec![
        ("repo-a".to_string(), single_node_canvas("a-module")),
        ("repo-b".to_string(), single_node_canvas("b-module")),
    ]);
    let mut positions: Vec<(i32, i32)> = mesh.nodes.iter().map(|n| (n.x, n.y)).collect();
    positions.sort();
    positions.dedup();
    assert_eq!(
        positions.len(),
        mesh.nodes.len(),
        "every node across every band must get a distinct position"
    );
    let module_xs: Vec<i32> = mesh
        .nodes
        .iter()
        .filter(|n| !n.id.starts_with("mesh-header-"))
        .map(|n| n.x)
        .collect();
    assert_eq!(module_xs.len(), 2);
    assert_ne!(module_xs[0], module_xs[1], "bands must not overlap in x");
}

#[test]
fn build_mesh_canvas_sums_overflow_across_bands() {
    let mut band = single_node_canvas("m");
    band.overflow_count = 3;
    let mesh = build_mesh_canvas(vec![
        ("repo-a".to_string(), band.clone()),
        ("repo-b".to_string(), band),
    ]);
    assert_eq!(mesh.overflow_count, 6);
}

#[test]
fn build_mesh_canvas_of_no_repos_is_an_empty_canvas() {
    let mesh = build_mesh_canvas(vec![]);
    assert!(mesh.nodes.is_empty());
    assert_eq!(mesh.overflow_count, 0);
}
