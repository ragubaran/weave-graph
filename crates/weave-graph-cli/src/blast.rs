//! `weave blast --base <ref>` (impl.md M2.12): PR blast-radius comment
//! mode. File-level touched-symbol mapping (documented precision limit —
//! every symbol `all_nodes` reports in a changed file counts as touched),
//! per-symbol `reachable_within` union, module-folded markdown at the
//! 200-node budget. Prints to stdout or a file; no GitHub networking —
//! the CI step pipes this into `gh pr comment`, `weave` never talks to
//! GitHub itself (Core Invariant 5).

use std::collections::HashSet;
use std::path::Path;

use crate::git;
use weave_graph_core::modules::{aggregate_file_edges, build_modules};
use weave_graph_core::{NodeId, Storage};

/// At or below this many impacted symbols the markdown lists them
/// individually; above it, module folding keeps the output bounded
/// (M1.8's 200-node budget discipline repointed at the PR comment).
const SYMBOL_BUDGET: usize = 200;

pub(crate) struct BlastReport {
    pub(crate) base: String,
    pub(crate) changed_files: Vec<String>,
    /// `(symbol, path, line_start)` of every impacted symbol, sorted.
    pub(crate) impacted: Vec<(String, String, u32)>,
    /// True when the markdown output folded by module to stay in budget.
    pub(crate) folded: bool,
    /// `(label, file count, impacted symbol count, member files)` for
    /// modules containing at least one impacted symbol.
    pub(crate) modules: Vec<(String, usize, usize, Vec<String>)>,
}

pub(crate) fn cmd_blast(
    root: &Path,
    base: &str,
    format: &str,
    out: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let report = compute(root, base)?;
    let text = match format {
        "json" => render_json(&report)?,
        "md" | "markdown" => render_markdown(&report),
        other => return Err(format!("unknown format `{other}` (expected md or json)").into()),
    };
    match out {
        Some(path) => {
            std::fs::write(path, text)?;
            println!("Blast report written to {}", path.display());
        }
        None => print!("{text}"),
    }
    Ok(())
}

/// One `Storage` + one `CsrGraph`, loaded once and reused across every
/// touched symbol's traversal — never a per-symbol reopen (M2.12's own
/// acceptance criterion).
fn compute(root: &Path, base: &str) -> Result<BlastReport, Box<dyn std::error::Error>> {
    let changed_files = git::blast_since(root, base)?;

    let (storage, _db_path) = crate::open_storage_for_read(root)?;
    let nodes = storage.all_nodes()?;
    let edges = storage.all_edges()?;
    let csr = weave_graph_core::CsrGraph::load(&storage)?;

    let changed: HashSet<&str> = changed_files.iter().map(String::as_str).collect();
    let touched: Vec<&weave_graph_core::Node> = nodes
        .iter()
        .filter(|n| changed.contains(n.path.as_str()))
        .collect();

    // File-level precision (v1 limit, documented): every symbol in a
    // changed file counts as touched; line-level hunk ranges are
    // follow-on scope.
    let mut impacted: HashSet<u32> = HashSet::new();
    for node in &touched {
        impacted.extend(csr.reachable_within(node.id, u32::MAX).iter());
    }
    let mut impacted_list: Vec<(String, String, u32)> = impacted
        .iter()
        .filter_map(|idx| nodes.get(*idx as usize))
        .map(|n| (n.symbol.clone(), n.path.clone(), n.line_start))
        .collect();
    impacted_list.sort();
    impacted_list.dedup();

    // Module folding for the markdown budget: fold only the modules that
    // actually contain impacted symbols.
    let file_of: std::collections::HashMap<NodeId, String> =
        nodes.iter().map(|n| (n.id, n.path.clone())).collect();
    let file_edges = aggregate_file_edges(&edges, &file_of);
    let modules = build_modules(&nodes, &file_edges);
    let impacted_paths: HashSet<&str> = impacted_list.iter().map(|(_, p, _)| p.as_str()).collect();
    let impacted_symbols_per_path: std::collections::HashMap<&str, usize> = {
        let mut m = std::collections::HashMap::new();
        for (_, path, _) in &impacted_list {
            *m.entry(path.as_str()).or_insert(0) += 1;
        }
        m
    };
    let folded_modules: Vec<(String, usize, usize, Vec<String>)> = modules
        .iter()
        .filter_map(|module| {
            let hit: Vec<&String> = module
                .files
                .iter()
                .filter(|f| impacted_paths.contains(f.as_str()))
                .collect();
            if hit.is_empty() {
                return None;
            }
            let symbols: usize = hit
                .iter()
                .map(|f| impacted_symbols_per_path[f.as_str()])
                .sum();
            let files: Vec<String> = hit.iter().map(|f| (*f).clone()).collect();
            Some((module.label.clone(), hit.len(), symbols, files))
        })
        .collect();

    Ok(BlastReport {
        base: base.to_string(),
        changed_files,
        impacted: impacted_list,
        folded: false,
        modules: folded_modules,
    })
}

fn render_markdown(report: &BlastReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("## Weave blast radius: `{}`...`HEAD`", report.base));
    lines.push(format!(
        "{} file(s) changed: {}",
        report.changed_files.len(),
        report.changed_files.join(", ")
    ));
    if report.changed_files.is_empty() {
        lines.push("_No file changes on the PR side of this range._".to_string());
        return lines.join("\n");
    }
    lines.push(format!("{} symbols impacted.", report.impacted.len()));

    if report.impacted.len() <= SYMBOL_BUDGET {
        // Per-symbol, grouped by file.
        let mut by_file: std::collections::BTreeMap<&str, Vec<&(String, String, u32)>> =
            std::collections::BTreeMap::new();
        for entry in &report.impacted {
            by_file.entry(entry.1.as_str()).or_default().push(entry);
        }
        lines.push("\n### Impacted symbols".to_string());
        for (path, symbols) in &by_file {
            lines.push(format!(
                "- `{path}`: {}",
                symbols
                    .iter()
                    .map(|(s, _, l)| format!("`{s}` (L{l})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let _ = report.folded;
    } else {
        // Module-folded: one line per impacted module stays inside the
        // 200-node budget regardless of repo size.
        lines.push(format!(
            "\n### Impacted modules (folded: {} symbols > {} budget)",
            report.impacted.len(),
            SYMBOL_BUDGET
        ));
        for (label, files, symbols, members) in &report.modules {
            lines.push(format!(
                "- **{label}** [{files} files, {symbols} impacted symbols]: {}",
                members.join(", ")
            ));
        }
    }
    lines.push(String::new());
    lines.push(
        "_File-level precision: every symbol in a changed file counts as touched \
         (v1 limit). Generated by `weave blast`; no GitHub networking._"
            .to_string(),
    );
    lines.join("\n")
}

fn render_json(report: &BlastReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&serde_json::json!({
        "base": report.base,
        "changed_files": report.changed_files,
        "impacted_symbols": report.impacted.iter()
            .map(|(s, p, l)| serde_json::json!({"symbol": s, "path": p, "line_start": l}))
            .collect::<Vec<_>>(),
        "impacted_count": report.impacted.len(),
        "folded_by_module": report.impacted.len() > SYMBOL_BUDGET,
        "modules": report.modules.iter()
            .map(|(label, files, symbols, members)| serde_json::json!({
                "label": label, "files": files, "impacted_symbols": symbols, "members": members
            }))
            .collect::<Vec<_>>(),
    }))
}
#[cfg(test)]
mod tests;
