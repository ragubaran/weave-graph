//! `provenance` feature render side (`impl.md` M2.3): turns provider-
//! signed `doc_links` rows into `weave report`/`weave export` output.
//! The CLI never attaches provenance — a host application wires the
//! real `ProvenanceProvider` and writes through the store API; weave
//! only renders what is already present, and renders nothing when no
//! row carries provenance.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use weave_graph_core::{Node, NodeId, Storage, StorageError};
use weave_graph_store_sqlite::{DocLinkProvenance, SqliteStorage};

#[derive(Serialize)]
pub(crate) struct DocProvenanceView {
    doc: String,
    target: String,
    section: String,
    kind: String,
    commit_hash: String,
    merkle_root: String,
    signature: String,
}

fn node_label(nodes: &HashMap<NodeId, Node>, id: NodeId) -> String {
    match nodes.get(&id) {
        Some(node) => format!("{} ({})", node.symbol, node.path),
        None => format!("(missing node {id})"),
    }
}

fn load_nodes(storage: &SqliteStorage) -> Result<HashMap<NodeId, Node>, StorageError> {
    Ok(storage
        .all_nodes()?
        .into_iter()
        .map(|node| (node.id, node))
        .collect())
}

fn to_view(row: &DocLinkProvenance, nodes: &HashMap<NodeId, Node>) -> DocProvenanceView {
    DocProvenanceView {
        doc: node_label(nodes, row.doc_id),
        target: node_label(nodes, row.target_node_id),
        section: row.section.clone(),
        kind: row.kind.clone(),
        commit_hash: row.provenance.commit_hash.clone(),
        merkle_root: row.provenance.merkle_root.clone(),
        signature: row.provenance.signature.clone(),
    }
}

/// The `WEAVE_REPORT.md` section for signed doc links, or `None` when
/// no `doc_links` row carries provenance — "render when present" is the
/// milestone's own constraint, so absence must leave the report
/// byte-identical to a default build's.
pub(crate) fn report_section(storage: &SqliteStorage) -> Result<Option<String>, StorageError> {
    let rows = storage.doc_links_with_provenance()?;
    if rows.is_empty() {
        return Ok(None);
    }
    let nodes = load_nodes(storage)?;
    let mut out = String::from("## Document Provenance\n\n");
    for row in &rows {
        let view = to_view(row, &nodes);
        out.push_str(&format!(
            "- `{}` -> `{}` ({} in \"{}\") — commit `{}`, merkle root `{}`, signature `{}`\n",
            view.doc,
            view.target,
            view.kind,
            view.section,
            view.commit_hash,
            view.merkle_root,
            view.signature
        ));
    }
    Ok(Some(out))
}

/// Export entries for the doc links touching `ids` — a doc link belongs
/// to a neighborhood when either endpoint is inside it.
pub(crate) fn export_entries(
    storage: &SqliteStorage,
    ids: &HashSet<NodeId>,
) -> Result<Vec<DocProvenanceView>, StorageError> {
    let nodes = load_nodes(storage)?;
    Ok(storage
        .doc_links_with_provenance()?
        .iter()
        .filter(|row| ids.contains(&row.doc_id) || ids.contains(&row.target_node_id))
        .map(|row| to_view(row, &nodes))
        .collect())
}

#[cfg(test)]
mod tests;
