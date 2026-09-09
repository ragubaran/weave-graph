use std::collections::HashSet;

use weave_graph_core::provenance::{MockProvenanceProvider, ProvenanceProvider};
use weave_graph_core::{Node, NodeId, Storage};
use weave_graph_store_sqlite::SqliteStorage;

use super::*;

fn node(symbol: &str, kind: &str) -> Node {
    Node {
        id: 0,
        repo_id: "local".to_string(),
        path: format!("src/{symbol}.rs"),
        symbol: symbol.to_string(),
        kind: kind.to_string(),
        line_start: 1,
        line_end: 3,
        signature: String::new(),
    }
}

/// Two code nodes plus one signed doc link from a doc note to the
/// first — the minimal shape `report_section`/`export_entries` render.
fn seeded() -> (SqliteStorage, NodeId, NodeId, NodeId) {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let doc_id = storage.upsert_node(&node("adr", "doc_note")).unwrap();
    let linked = storage.upsert_node(&node("helper", "function")).unwrap();
    let unlinked = storage.upsert_node(&node("unrelated", "function")).unwrap();

    let provider = MockProvenanceProvider::new();
    let record = provider.attach(doc_id, "deadbeef").unwrap();
    storage
        .upsert_doc_link(
            doc_id,
            "rationale",
            linked,
            "EXPLAINS_RATIONALE",
            Some(&record),
        )
        .unwrap();
    (storage, doc_id, linked, unlinked)
}

fn ids_by_symbol(storage: &SqliteStorage, symbol: &str) -> NodeId {
    storage
        .all_nodes()
        .unwrap()
        .into_iter()
        .find(|n| n.symbol == symbol)
        .map(|n| n.id)
        .unwrap()
}

#[test]
fn report_section_renders_signed_links_with_their_records() {
    let (storage, doc_id, linked, _) = seeded();

    let section = report_section(&storage).unwrap().unwrap();

    assert!(section.contains("## Document Provenance"));
    assert!(section.contains("adr (src/adr.rs)"));
    assert!(section.contains("helper (src/helper.rs)"));
    assert!(section.contains("EXPLAINS_RATIONALE"));
    assert!(section.contains("rationale"));
    assert!(section.contains("deadbeef"));
    assert_eq!(doc_id, ids_by_symbol(&storage, "adr"));
    assert_eq!(linked, ids_by_symbol(&storage, "helper"));
}

#[test]
fn report_section_is_none_when_no_link_carries_provenance() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let doc_id = storage.upsert_node(&node("adr", "doc_note")).unwrap();
    let target = storage.upsert_node(&node("helper", "function")).unwrap();
    storage
        .upsert_doc_link(doc_id, "rationale", target, "EXPLAINS_RATIONALE", None)
        .unwrap();

    assert!(report_section(&storage).unwrap().is_none());
}

#[test]
fn export_entries_keep_links_touching_the_neighborhood() {
    let (storage, _, linked, unlinked) = seeded();

    let in_neighborhood = HashSet::from([linked]);
    let entries = export_entries(&storage, &in_neighborhood).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].doc.contains("adr"));
    assert!(entries[0].target.contains("helper"));
    assert_eq!(entries[0].commit_hash, "deadbeef");

    let outside = HashSet::from([unlinked]);
    assert!(export_entries(&storage, &outside).unwrap().is_empty());
}

#[test]
fn export_entries_keep_links_whose_doc_note_is_in_the_neighborhood() {
    let (storage, doc_id, _, _) = seeded();

    let entries = export_entries(&storage, &HashSet::from([doc_id])).unwrap();
    assert_eq!(entries.len(), 1, "doc-side endpoint also matches");
}
