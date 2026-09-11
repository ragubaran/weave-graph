use weave_graph_core::{Edge, Node};
use weave_graph_store_sqlite::SqliteStorage;

use super::*;

fn node(path: &str, symbol: &str) -> Node {
    Node {
        id: 0,
        repo_id: "local".into(),
        path: path.into(),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start: 1,
        line_end: 3,
        signature: format!("fn {symbol}()"),
    }
}

fn edge(source_id: NodeId, target_id: NodeId) -> Edge {
    Edge {
        id: 0,
        source_id,
        target_id,
        kind: "CALLS_EXACT".into(),
        weight: 1.0,
    }
}

#[test]
fn aggregate_file_edges_ignores_self_file_edges_and_sums_cross_file_ones() {
    let nodes = vec![node("a.rs", "a1"), node("a.rs", "a2"), node("b.rs", "b1")];
    let ids: Vec<NodeId> = (1..=3).collect();
    let file_of: HashMap<NodeId, String> = ids
        .iter()
        .zip(&nodes)
        .map(|(&id, n)| (id, n.path.clone()))
        .collect();
    let edges = vec![
        edge(ids[0], ids[1]), // a.rs -> a.rs, dropped
        edge(ids[0], ids[2]), // a.rs -> b.rs
        edge(ids[2], ids[0]), // b.rs -> a.rs, same unordered pair
    ];
    let weights = aggregate_file_edges(&edges, &file_of);
    assert_eq!(weights.len(), 1);
    assert_eq!(
        weights.get(&("a.rs".to_string(), "b.rs".to_string())),
        Some(&2.0)
    );
}

#[test]
fn build_modules_groups_densely_connected_files_together() {
    let nodes = vec![node("a.rs", "a"), node("b.rs", "b"), node("c.rs", "c")];
    let mut file_edges = HashMap::new();
    file_edges.insert(("a.rs".to_string(), "b.rs".to_string()), 5.0);
    // c.rs is isolated — no edges to a.rs/b.rs.
    let modules = build_modules(&nodes, &file_edges);
    let module_of = |path: &str| {
        modules
            .iter()
            .find(|m| m.files.iter().any(|f| f == path))
            .map(|m| m.id)
    };
    assert_eq!(module_of("a.rs"), module_of("b.rs"));
    assert_ne!(module_of("a.rs"), module_of("c.rs"));
}

#[test]
fn repo_canvas_has_one_node_per_distinct_repo_id() {
    let nodes = vec![node("a.rs", "a"), node("b.rs", "b")];
    let canvas = repo_canvas(&nodes);
    assert_eq!(canvas.nodes.len(), 1, "both nodes share repo_id \"local\"");
}

#[test]
fn modules_canvas_stays_within_budget_on_a_synthetic_1000_module_graph() {
    let modules: Vec<Module> = (0..1000)
        .map(|i| Module {
            id: i,
            label: format!("module{i}"),
            files: vec![format!("m{i}/a.rs")],
        })
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let (canvas, overflow) = modules_canvas(&modules, &HashMap::new(), dir.path());

    assert!(
        canvas.nodes.len() <= NODE_BUDGET,
        "top-level canvas must respect the {NODE_BUDGET}-node budget, got {}",
        canvas.nodes.len()
    );
    assert!(
        !overflow.is_empty(),
        "1000 modules must overflow into a sub-canvas"
    );
    assert!(
        canvas.nodes.iter().any(|n| n.node_type == "file"),
        "overflow link node must be present"
    );

    let total_in_overflow: usize = overflow.iter().map(|(_, c)| c.nodes.len()).sum();
    assert_eq!(
        canvas.nodes.len() - 1 + total_in_overflow,
        1000,
        "every module must appear exactly once, either top-level or in overflow"
    );
}

#[test]
fn modules_canvas_has_no_overflow_when_under_budget() {
    let modules: Vec<Module> = (0..5)
        .map(|i| Module {
            id: i,
            label: format!("module{i}"),
            files: vec![format!("m{i}/a.rs")],
        })
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let (canvas, overflow) = modules_canvas(&modules, &HashMap::new(), dir.path());
    assert_eq!(canvas.nodes.len(), 5);
    assert!(overflow.is_empty());
}

fn seeded_storage() -> (tempfile::TempDir, SqliteStorage) {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = SqliteStorage::open(&dir.path().join("graph.db")).unwrap();
    let a = storage.upsert_node(&node("src/a.rs", "helper")).unwrap();
    let b = storage.upsert_node(&node("src/b.rs", "caller")).unwrap();
    storage.upsert_edge(&edge(b, a)).unwrap();
    (dir, storage)
}

#[test]
fn generate_writes_a_report_and_canvases_that_are_all_valid_json() {
    let (dir, storage) = seeded_storage();
    let out_dir = dir.path().join("out");
    let db_path = dir.path().join("graph.db");

    let paths = generate(dir.path(), &out_dir, &db_path, &storage, None).unwrap();

    assert!(paths.report_md.exists());
    let report = fs::read_to_string(&paths.report_md).unwrap();
    assert!(report.contains("Status: Static Snapshot"));
    assert!(report.contains("Weave Graph Report"));

    assert!(!paths.canvas_files.is_empty());
    for canvas_path in &paths.canvas_files {
        let content = fs::read_to_string(canvas_path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("{canvas_path:?} is not valid JSON: {e}"));
        assert!(value["nodes"].is_array());
        assert!(value["edges"].is_array());
    }
}

#[test]
fn generate_appends_the_doc_provenance_section_only_when_given_one() {
    let (dir, storage) = seeded_storage();
    let out_dir = dir.path().join("out");
    let db_path = dir.path().join("graph.db");

    let without = generate(dir.path(), &out_dir, &db_path, &storage, None).unwrap();
    let report = fs::read_to_string(&without.report_md).unwrap();
    assert!(!report.contains("Document Provenance"));

    let with = generate(
        dir.path(),
        &out_dir,
        &db_path,
        &storage,
        Some("## Document Provenance\n\n- signed link"),
    )
    .unwrap();
    let report = fs::read_to_string(&with.report_md).unwrap();
    assert!(report.contains("## Document Provenance"));
    assert!(report.contains("- signed link"));
}
