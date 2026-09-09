//! `provenance` feature (`impl.md` M2.3): persistence for
//! Merkle-signed note links. A host application wires a real
//! `ProvenanceProvider`, calls `attach`, and writes the record here;
//! `weave report`/`weave export` only render what these reads return.

use rusqlite::{Connection, params};
use weave_graph_core::NodeId;
use weave_graph_core::StorageError;
use weave_graph_core::provenance::Provenance;

/// One `doc_links` row that carries provenance, record reassembled in
/// full so `ProvenanceProvider::verify` can recompute the root after a
/// store round-trip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocLinkProvenance {
    pub doc_link_id: u32,
    pub doc_id: NodeId,
    pub target_node_id: NodeId,
    pub section: String,
    pub kind: String,
    pub provenance: Provenance,
}

fn backend_err(e: rusqlite::Error) -> StorageError {
    StorageError::Backend(e.to_string())
}

/// Insert-or-replace one `doc_links` row keyed on
/// `(doc_id, section, target_node_id, kind)` — the table has no unique
/// natural-key index, so replacement is an explicit delete-then-insert
/// inside one transaction (same shape as `upsert_contract`). `None`
/// records an unsigned link; unsigned rows never surface from
/// `doc_links_with_provenance`.
pub(crate) fn upsert_doc_link(
    conn: &mut Connection,
    doc_id: NodeId,
    section: &str,
    target_node_id: NodeId,
    kind: &str,
    provenance: Option<&Provenance>,
) -> Result<(), StorageError> {
    if let Some(record) = provenance
        && record.doc_id != doc_id
    {
        return Err(StorageError::Backend(format!(
            "provenance doc_id {} does not match doc_links doc_id {doc_id}",
            record.doc_id
        )));
    }
    let tx = conn.transaction().map_err(backend_err)?;
    tx.execute(
        "DELETE FROM doc_links WHERE doc_id = ?1 AND section = ?2 AND target_node_id = ?3 AND kind = ?4",
        params![doc_id, section, target_node_id, kind],
    )
    .map_err(backend_err)?;
    tx.execute(
        "INSERT INTO doc_links (doc_id, section, target_node_id, kind, provenance_commit, provenance_hash, provenance_signature)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            doc_id,
            section,
            target_node_id,
            kind,
            provenance.map(|p| p.commit_hash.as_str()),
            provenance.map(|p| p.merkle_root.as_str()),
            provenance.map(|p| p.signature.as_str()),
        ],
    )
    .map_err(backend_err)?;
    tx.commit().map_err(backend_err)
}

pub(crate) fn doc_links_with_provenance(
    conn: &Connection,
) -> Result<Vec<DocLinkProvenance>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, doc_id, target_node_id, section, kind, provenance_commit, provenance_hash, provenance_signature
             FROM doc_links
             WHERE provenance_commit IS NOT NULL AND provenance_hash IS NOT NULL AND provenance_signature IS NOT NULL
             ORDER BY id",
        )
        .map_err(backend_err)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(DocLinkProvenance {
                doc_link_id: row.get::<_, i64>(0)? as u32,
                doc_id: row.get::<_, i64>(1)? as NodeId,
                target_node_id: row.get::<_, i64>(2)? as NodeId,
                section: row.get(3)?,
                kind: row.get(4)?,
                provenance: Provenance {
                    doc_id: row.get::<_, i64>(1)? as NodeId,
                    commit_hash: row.get(5)?,
                    merkle_root: row.get(6)?,
                    signature: row.get(7)?,
                },
            })
        })
        .map_err(backend_err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(backend_err)
}

#[cfg(test)]
mod tests;
