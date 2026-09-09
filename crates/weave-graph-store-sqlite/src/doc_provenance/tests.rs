use weave_graph_core::provenance::{MockProvenanceProvider, ProvenanceProvider, VerifyResult};
use weave_graph_core::{Node, Storage};

use super::*;
use crate::SqliteStorage;

fn node(symbol: &str, kind: &str) -> Node {
    Node {
        id: 0,
        repo_id: "local".to_string(),
        path: format!("{symbol}.md"),
        symbol: symbol.to_string(),
        kind: kind.to_string(),
        line_start: 0,
        line_end: 0,
        signature: String::new(),
    }
}

fn seeded() -> SqliteStorage {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage.upsert_node(&node("note", "doc_note")).unwrap();
    storage.upsert_node(&node("verify", "function")).unwrap();
    storage
}

fn ids(storage: &SqliteStorage, symbol: &str) -> NodeId {
    storage
        .all_nodes()
        .unwrap()
        .into_iter()
        .find(|n| n.symbol == symbol)
        .map(|n| n.id)
        .unwrap()
}

#[test]
fn round_trip_preserves_the_record_and_still_verifies() {
    let mut storage = seeded();
    let (doc_id, target_id) = (ids(&storage, "note"), ids(&storage, "verify"));
    let provider = MockProvenanceProvider::new();
    let record = provider.attach(doc_id, "abc123").unwrap();

    storage
        .upsert_doc_link(
            doc_id,
            "intro",
            target_id,
            "EXPLAINS_RATIONALE",
            Some(&record),
        )
        .unwrap();

    let rows = storage.doc_links_with_provenance().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].doc_id, doc_id);
    assert_eq!(rows[0].target_node_id, target_id);
    assert_eq!(rows[0].section, "intro");
    assert_eq!(rows[0].kind, "EXPLAINS_RATIONALE");
    assert_eq!(rows[0].provenance, record);
    assert_eq!(provider.verify(&rows[0].provenance), VerifyResult::Verified);
}

#[test]
fn unsigned_links_are_not_returned() {
    let mut storage = seeded();
    let (doc_id, target_id) = (ids(&storage, "note"), ids(&storage, "verify"));

    storage
        .upsert_doc_link(doc_id, "intro", target_id, "LINKS_TO", None)
        .unwrap();

    assert!(storage.doc_links_with_provenance().unwrap().is_empty());
}

#[test]
fn re_upsert_on_the_same_natural_key_replaces_the_row() {
    let mut storage = seeded();
    let (doc_id, target_id) = (ids(&storage, "note"), ids(&storage, "verify"));
    let provider = MockProvenanceProvider::new();
    let first = provider.attach(doc_id, "first").unwrap();
    let second = provider.attach(doc_id, "second").unwrap();

    storage
        .upsert_doc_link(doc_id, "intro", target_id, "LINKS_TO", Some(&first))
        .unwrap();
    storage
        .upsert_doc_link(doc_id, "intro", target_id, "LINKS_TO", Some(&second))
        .unwrap();

    let rows = storage.doc_links_with_provenance().unwrap();
    assert_eq!(rows.len(), 1, "same natural key must not duplicate rows");
    assert_eq!(rows[0].provenance, second);
}

#[test]
fn a_record_for_a_different_doc_id_is_rejected() {
    let mut storage = seeded();
    let (doc_id, target_id) = (ids(&storage, "note"), ids(&storage, "verify"));
    let provider = MockProvenanceProvider::new();
    let mismatched = provider.attach(doc_id + 1, "abc123").unwrap();

    let err = storage
        .upsert_doc_link(doc_id, "intro", target_id, "LINKS_TO", Some(&mismatched))
        .unwrap_err();

    assert!(err.to_string().contains("does not match"));
}

#[test]
fn distinct_natural_keys_produce_distinct_rows() {
    let mut storage = seeded();
    let (doc_id, target_id) = (ids(&storage, "note"), ids(&storage, "verify"));
    let provider = MockProvenanceProvider::new();
    let record = provider.attach(doc_id, "abc123").unwrap();

    storage
        .upsert_doc_link(doc_id, "intro", target_id, "LINKS_TO", Some(&record))
        .unwrap();
    storage
        .upsert_doc_link(doc_id, "summary", target_id, "LINKS_TO", Some(&record))
        .unwrap();

    assert_eq!(storage.doc_links_with_provenance().unwrap().len(), 2);
}
