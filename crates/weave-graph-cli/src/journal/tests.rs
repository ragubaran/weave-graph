use weave_graph_core::{Edge, Node, Storage};
use weave_graph_store_sqlite::SqliteStorage;

use super::*;

fn node(symbol: &str, kind: &str, path: &str, id: u32) -> Node {
    Node {
        id,
        repo_id: "local".to_string(),
        path: path.to_string(),
        symbol: symbol.to_string(),
        kind: kind.to_string(),
        line_start: 1,
        line_end: 3,
        signature: String::new(),
    }
}

fn edge(source_id: u32, target_id: u32) -> Edge {
    Edge {
        id: 0,
        source_id,
        target_id,
        kind: "CALLS_EXACT".to_string(),
        weight: 1.0,
    }
}

#[test]
fn render_covers_files_symbols_blast_and_docs() {
    let nodes = vec![node("helper", "function", "src/a.rs", 1)];
    let changed = vec!["src/a.rs".to_string()];
    let touched: Vec<&Node> = nodes.iter().filter(|n| n.id == 1).collect();
    let mut blast = std::collections::HashSet::new();
    blast.insert(1u32);
    let referencing = vec!["docs/adr.md (references helper)".to_string()];

    let out = render("HEAD~1", &changed, &touched, &blast, &nodes, &referencing);

    assert!(out.contains("# Weave Journal"));
    assert!(out.contains("## Changed since HEAD~1"));
    assert!(out.contains("- src/a.rs"));
    assert!(out.contains("- helper src/a.rs:1-3"));
    assert!(out.contains("- 1 symbol reachable outbound"));
    assert!(out.contains("- docs/adr.md (references helper)"));
}

#[test]
fn render_handles_the_empty_working_tree() {
    let nodes: Vec<Node> = Vec::new();
    let out = render("main", &[], &[], &std::collections::HashSet::new(), &nodes, &[]);
    assert!(out.contains("(working tree clean"));
    assert!(out.contains("(none indexed"));
    assert!(out.contains("- 0 symbols reachable"));
    assert!(out.contains("(no doc_note references"));
}

#[test]
fn doc_note_inbound_edges_surface_as_referencing_docs() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage.upsert_node(&node("helper", "function", "src/a.rs", 1)).unwrap();
    storage.upsert_node(&node("adr", "doc_note", "docs/adr.md", 2)).unwrap();
    storage.upsert_edge(&edge(2, 1)).unwrap();
    storage.upsert_node(&node("plain_caller", "function", "src/b.rs", 3)).unwrap();
    storage.upsert_edge(&edge(3, 1)).unwrap();

    let nodes = storage.all_nodes().unwrap();
    let referencing: Vec<String> = nodes
        .iter()
        .filter(|n| n.id == 1)
        .flat_map(|n| storage.get_callers(n.id).unwrap_or_default())
        .filter_map(|e| {
            let source = nodes.iter().find(|n| n.id == e.source_id)?;
            (source.kind == "doc_note").then(|| format!("{} (references {})", source.path, source.symbol))
        })
        .collect();

    assert_eq!(referencing, vec!["docs/adr.md (references adr)".to_string()]);
}

#[test]
fn journal_with_explicit_bad_ref_fails_clearly() {
    let dir = tempfile::tempdir().unwrap();
    // No git repo, no index: the git diff step must fail with the
    // --since hint, never a panic or a bare git error.
    let err = cmd_journal(dir.path(), Some("no-such-ref")).unwrap_err();
    assert!(err.to_string().contains("--since"), "{err}");
}
