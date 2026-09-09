//! `weave journal` (`impl.md` M2.4.5, `suges-slm.md` §2.4): combines the
//! git diff with the graph delta into a structured changelog — what
//! changed, which symbols were affected, the blast radius, and which
//! docs reference the changed code. Every symbol/line figure comes from
//! the index; there is no model in this path at all.

use std::collections::HashSet;
use std::path::Path;

use weave_graph_core::{CsrGraph, NodeId, Storage};

use crate::git;

pub(crate) fn cmd_journal(
    root: &Path,
    since: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let base = match since {
        Some(reference) => reference.to_string(),
        None => "HEAD~1".to_string(),
    };
    // Fail fast on the git step before opening the index — a missing
    // base ref is a --since problem, not a graph-database problem.
    let changed = git::changed_since(root, &base).ok_or_else(|| {
        format!("could not diff against {base:?} — pass --since <ref> explicitly (a single-commit repo has no HEAD~1)")
    })?;
    let (storage, _db_path) = crate::open_storage_for_read(root)?;
    let nodes = storage.all_nodes().map_err(|e| e.to_string())?;
    let csr = CsrGraph::load(&storage).map_err(|e| e.to_string())?;

    let touched: Vec<&weave_graph_core::Node> = nodes
        .iter()
        .filter(|n| changed.iter().any(|path| path == &n.path))
        .collect();
    let touched_ids: HashSet<NodeId> = touched.iter().map(|n| n.id).collect();

    // One CSR/Storage reuse for the whole union — never per-symbol.
    let mut blast: HashSet<NodeId> = HashSet::new();
    for &id in &touched_ids {
        blast.extend(
            csr.reachable_within(id, u32::MAX)
                .iter()
                .map(|i| nodes.get(i as usize).map(|n| n.id).unwrap_or(id)),
        );
    }

    // Docs referencing changed code: inbound edges from doc_note nodes.
    let kind_of: std::collections::HashMap<NodeId, String> =
        nodes.iter().map(|n| (n.id, n.kind.clone())).collect();
    let referencing: Vec<String> = touched_ids
        .iter()
        .flat_map(|&id| storage.get_callers(id).unwrap_or_default())
        .filter_map(|edge| {
            let source = nodes.iter().find(|n| n.id == edge.source_id)?;
            (kind_of.get(&source.id).map(|k| k.as_str()) == Some("doc_note")).then(|| {
                format!(
                    "{} (references {})",
                    source.path,
                    symbol_of(&nodes, edge.target_id)
                )
            })
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    print!(
        "{}",
        render(&base, &changed, &touched, &blast, &nodes, &referencing)
    );
    Ok(())
}

fn symbol_of(nodes: &[weave_graph_core::Node], id: NodeId) -> String {
    nodes
        .iter()
        .find(|n| n.id == id)
        .map(|n| n.symbol.clone())
        .unwrap_or_else(|| format!("node {id}"))
}

fn render(
    base: &str,
    changed: &[String],
    touched: &[&weave_graph_core::Node],
    blast: &HashSet<NodeId>,
    nodes: &[weave_graph_core::Node],
    referencing: &[String],
) -> String {
    let mut out = String::from("# Weave Journal\n\n");
    out.push_str(&format!("## Changed since {base}\n\n"));
    if changed.is_empty() {
        out.push_str("- (working tree clean relative to the base)\n");
    }
    for path in changed {
        out.push_str(&format!("- {path}\n"));
    }
    out.push_str("\n## Affected symbols\n\n");
    if touched.is_empty() {
        out.push_str("- (none indexed in the changed files)\n");
    }
    for node in touched {
        out.push_str(&format!(
            "- {} {}:{}-{}\n",
            node.symbol, node.path, node.line_start, node.line_end
        ));
    }
    out.push_str("\n## Blast radius\n\n");
    out.push_str(&format!(
        "- {} symbol{} reachable outbound from the changed set\n",
        blast.len(),
        if blast.len() == 1 { "" } else { "s" }
    ));
    out.push_str("\n## Referencing docs\n\n");
    if referencing.is_empty() {
        out.push_str("- (no doc_note references the changed code)\n");
    }
    for line in referencing {
        out.push_str(&format!("- {line}\n"));
    }
    let _ = nodes;
    out
}

#[cfg(test)]
mod tests;
