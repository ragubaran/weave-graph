//! `federation` feature (`plan.md` §2.3, `impl.md` M2.1): `weave link`
//! composes two independently-indexed repos' already-built `graph.db`
//! files into one addressable graph under composite keys
//! (`[repo_id]::[path]::[symbol]`), then runs Tarjan's SCC over it to
//! surface circular cross-repo dependencies. Purely local — reads two
//! SQLite files plus each repo's own raw source, and writes a report; no
//! networking crate anywhere in this module's dependency tree.
//!
//! **Cross-repo edges are real, not simulated**: each repo's own
//! already-resolved edges (from its `graph.db`) only ever reference that
//! repo's own node ids — a repo's `IMPORTS`/call reference that couldn't
//! resolve *within* that repo was silently dropped at indexing time (M1.2's
//! "never a dangling target" rule), and the raw target text isn't
//! persisted once dropped. So composing already-indexed graphs alone can
//! never surface a cross-repo dependency. This module re-parses both
//! repos' raw source (`index::parse_all`, the same function `weave index`
//! itself uses) and retries each repo's otherwise-unresolved references
//! against the *other* repo's freshly-rebuilt `ProjectIndex`
//! (`ProjectIndex::resolve_cross_repo`) — the same by-short-name
//! resolution this project already uses everywhere, just given a second
//! index to fall back to.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use weave_graph_core::federation::{composite_key, tarjan_scc};
use weave_graph_core::{Node, Storage};
use weave_graph_parse::{Language, ParsedFile, ProjectIndex, contract, moniker};

use crate::discover_files;
use crate::index::parse_all;
use crate::open_storage_for_read;

struct RepoGraph {
    label: String,
    nodes: Vec<Node>,
    edges: Vec<(u32, u32)>,
    project_index: ProjectIndex,
    parsed_files: Vec<(PathBuf, ParsedFile)>,
}

fn repo_label(repo_root: &Path) -> String {
    repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| repo_root.to_string_lossy().to_string())
}

fn load_repo(root: &Path) -> Result<RepoGraph, Box<dyn std::error::Error>> {
    let (storage, _db_path) = open_storage_for_read(root)?;
    let nodes = storage.all_nodes()?;
    let edges = storage
        .all_edges()?
        .into_iter()
        .map(|e| (e.source_id, e.target_id))
        .collect();

    let files = discover_files(root);
    let (project_index, parsed_files) = parse_all(root, &files);

    Ok(RepoGraph {
        label: repo_label(root),
        nodes,
        edges,
        project_index,
        parsed_files,
    })
}

pub(crate) fn cmd_link(repo_a: &Path, repo_b: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let a = load_repo(repo_a)?;
    let b = load_repo(repo_b)?;
    if a.label == b.label {
        return Err(format!(
            "repo_a and repo_b both resolve to the label '{}' — federation needs two \
             distinguishable repos; rename one or pass a differently-named path",
            a.label
        )
        .into());
    }

    let repos = [&a, &b];
    let mut composite_ids: HashMap<(u8, u32), u32> = HashMap::new();
    let mut moniker_ids: HashMap<(u8, String), u32> = HashMap::new();
    let mut composite_keys: Vec<String> = Vec::new();
    for (r, graph) in repos.iter().enumerate() {
        for node in &graph.nodes {
            let idx = composite_keys.len() as u32;
            composite_ids.insert((r as u8, node.id), idx);
            moniker_ids.insert((r as u8, moniker::build(&node.path, &node.symbol)), idx);
            composite_keys.push(composite_key(&graph.label, &node.path, &node.symbol));
        }
    }

    let mut composite_edges: Vec<(u32, u32)> = Vec::new();
    let mut repo_local_count = 0usize;
    for (r, graph) in repos.iter().enumerate() {
        for &(src, tgt) in &graph.edges {
            if let (Some(&u), Some(&v)) = (
                composite_ids.get(&(r as u8, src)),
                composite_ids.get(&(r as u8, tgt)),
            ) {
                composite_edges.push((u, v));
                repo_local_count += 1;
            }
        }
    }

    let cross_repo_edges = resolve_cross_repo_edges(&a, &b, &moniker_ids);
    let cross_repo_count = cross_repo_edges.len();
    composite_edges.extend(cross_repo_edges);

    let all_ids: Vec<u32> = (0..composite_keys.len() as u32).collect();
    let cycles: Vec<Vec<u32>> = tarjan_scc(&all_ids, &composite_edges)
        .into_iter()
        .filter(|c| c.len() > 1)
        .collect();

    let report = render_report(
        &a,
        &b,
        &composite_keys,
        repo_local_count,
        cross_repo_count,
        &cycles,
    );
    println!("{report}");

    let report_path = repo_a.join(".weave").join("federation-report.md");
    if report_path.parent().is_some_and(Path::exists) {
        fs::write(&report_path, &report)?;
        println!("Written to {}", report_path.display());
    }

    // M2.2: record each repo's boundary contract as the other repo's
    // expectation, so `weave check-contracts` can detect divergence later.
    let hash_a = repo_contract_hash(&a)?;
    let hash_b = repo_contract_hash(&b)?;
    crate::contracts::record_expectations(repo_a, repo_b, &hash_a, &hash_b, None, None)?;
    println!(
        "Recorded contract expectations: {} = {}, {} = {}",
        a.label, hash_a, b.label, hash_b
    );
    Ok(())
}

/// `weave link` with fewer than two explicit paths (impl.md M2.1's L7 gap):
/// the partner comes from this repo's `[federation] linked_repos` config.
/// Exactly one linked repo must be configured — ambiguity is an error, not
/// a guess.
pub(crate) fn cmd_link_from_config(
    root: &Path,
    partner: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = root.join(".weave").join("config.toml");
    let configured: Vec<PathBuf> = crate::config::read_linked_repos(&config_path)
        .into_iter()
        .map(|p| if p.is_absolute() { p } else { root.join(p) })
        .collect();

    let partner = match partner {
        Some(explicit) => explicit.to_path_buf(),
        None if configured.len() == 1 => configured[0].clone(),
        None if configured.is_empty() => {
            return Err(format!(
                "No linked repos configured in {} ([federation] linked_repos). \
                 Pass both repo paths explicitly, e.g. `weave link ../a ../b`.",
                config_path.display()
            )
            .into());
        }
        None => {
            return Err(format!(
                "{} linked repos configured in {}; pass the one to link explicitly, \
                 e.g. `weave link {}`.",
                configured.len(),
                config_path.display(),
                configured[0].display()
            )
            .into());
        }
    };
    cmd_link(root, &partner)
}

/// Whole-repo contract hash from already-parsed sources (`impl.md` M2.2):
/// every file's exported signatures, canonicalized and SHA-256'd. Reuses the
/// `parse_all` output `load_repo` already produced — no second file read.
fn repo_contract_hash(graph: &RepoGraph) -> Result<String, Box<dyn std::error::Error>> {
    let mut entries = Vec::new();
    for (path, parsed) in &graph.parsed_files {
        // `from_path` dispatches on extension, so the absolute path
        // `parse_all` produced needs no rewriting here.
        let Some(language) = Language::from_path(path) else {
            continue;
        };
        entries.extend(contract::exported_entries(language, parsed));
    }
    Ok(contract::hash_entries(entries))
}

/// Retries every reference either repo's own indexing left unresolved
/// against the *other* repo's `ProjectIndex`, and maps whatever resolves
/// into the shared composite id space via each endpoint's moniker. An
/// endpoint moniker with no matching node (e.g. a symbol kind the current
/// extractor doesn't index) is dropped, never fabricated.
fn resolve_cross_repo_edges(
    a: &RepoGraph,
    b: &RepoGraph,
    moniker_ids: &HashMap<(u8, String), u32>,
) -> Vec<(u32, u32)> {
    let mut edges = Vec::new();
    for (_path, parsed) in &a.parsed_files {
        for edge in a.project_index.resolve_cross_repo(parsed, &b.project_index) {
            if let (Some(&u), Some(&v)) = (
                moniker_ids.get(&(0u8, edge.source_moniker)),
                moniker_ids.get(&(1u8, edge.target_moniker)),
            ) {
                edges.push((u, v));
            }
        }
    }
    for (_path, parsed) in &b.parsed_files {
        for edge in b.project_index.resolve_cross_repo(parsed, &a.project_index) {
            if let (Some(&u), Some(&v)) = (
                moniker_ids.get(&(1u8, edge.source_moniker)),
                moniker_ids.get(&(0u8, edge.target_moniker)),
            ) {
                edges.push((u, v));
            }
        }
    }
    edges
}

fn render_report(
    a: &RepoGraph,
    b: &RepoGraph,
    composite_keys: &[String],
    repo_local_edges: usize,
    cross_repo_edges: usize,
    cycles: &[Vec<u32>],
) -> String {
    let mut report = format!(
        "# Federation Report: {} + {}\n\n\
         - {} composite nodes ({} from {}, {} from {})\n\
         - {} composite edges ({} repo-local, {} newly resolved cross-repo)\n\
         - {} circular dependency group(s) detected\n",
        a.label,
        b.label,
        composite_keys.len(),
        a.nodes.len(),
        a.label,
        b.nodes.len(),
        b.label,
        repo_local_edges + cross_repo_edges,
        repo_local_edges,
        cross_repo_edges,
        cycles.len(),
    );
    for cycle in cycles {
        report.push_str("  - ");
        let names: Vec<&str> = cycle
            .iter()
            .map(|&i| composite_keys[i as usize].as_str())
            .collect();
        report.push_str(&names.join(" -> "));
        report.push('\n');
    }
    report
}

#[cfg(test)]
mod tests;
