//! `federation` feature: `weave link` composes two independently-indexed
//! repos' already-built `graph.db` files into one addressable graph under
//! composite keys (`[repo_id]::[path]::[symbol]`), then runs Tarjan's SCC
//! over it to surface circular cross-repo dependencies. Purely local —
//! reads two SQLite files plus each repo's own raw source, and writes a
//! report; no networking crate anywhere in this module's dependency tree.
//!
//! **Cross-repo edges are real, not simulated**: each repo's own
//! already-resolved edges (from its `graph.db`) only ever reference that
//! repo's own node ids — a repo's `IMPORTS`/call reference that couldn't
//! resolve *within* that repo was silently dropped at indexing time (the
//! "never a dangling target" rule Core Invariant 3 requires), and the raw
//! target text isn't persisted once dropped. So composing already-indexed
//! graphs alone can
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
use weave_graph_core::{Edge, Node, Storage};
use weave_graph_parse::{Language, ParsedFile, ProjectIndex, contract, moniker};
use weave_graph_store_sqlite::SqliteStorage;

use crate::discover_files;
use crate::index::parse_all;
use crate::open_storage_for_read;

pub(crate) struct RepoGraph {
    pub(crate) label: String,
    pub(crate) nodes: Vec<Node>,
    edges: Vec<(u32, u32)>,
    pub(crate) project_index: ProjectIndex,
    pub(crate) parsed_files: Vec<(PathBuf, ParsedFile)>,
}

#[cfg(feature = "rbac")]
struct FederatedRbac {
    partner_label: String,
    primary: weave_graph_core::rbac::RbacGuard,
    partner: weave_graph_core::rbac::RbacGuard,
}

#[cfg(feature = "rbac")]
impl FederatedRbac {
    fn new(repo_a: &Path, repo_b: &Path, subject: &str) -> Self {
        Self {
            partner_label: repo_label(repo_b),
            primary: crate::rbac::guard_for(repo_a, Some(subject)),
            partner: crate::rbac::guard_for(repo_b, Some(subject)),
        }
    }

    fn guard<'a>(&'a self, node: &Node) -> &'a weave_graph_core::rbac::RbacGuard {
        if node.repo_id == self.partner_label {
            &self.partner
        } else {
            &self.primary
        }
    }

    fn mask(&self, node: &Node) -> Node {
        self.guard(node).mask_node(node)
    }

    fn visible(&self, node: &Node) -> bool {
        self.guard(node).visible(node)
    }
}

fn repo_label(repo_root: &Path) -> String {
    repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| repo_root.to_string_lossy().to_string())
}

pub(crate) fn load_repo(root: &Path) -> Result<RepoGraph, Box<dyn std::error::Error>> {
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
    let mut composite_nodes: Vec<Node> = Vec::new();
    for (r, graph) in repos.iter().enumerate() {
        for node in &graph.nodes {
            let idx = composite_keys.len() as u32;
            composite_ids.insert((r as u8, node.id), idx);
            moniker_ids.insert((r as u8, moniker::build(&node.path, &node.symbol)), idx);
            composite_keys.push(composite_key(&graph.label, &node.path, &node.symbol));
            composite_nodes.push(Node {
                id: idx,
                repo_id: graph.label.clone(),
                ..node.clone()
            });
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

    let local_edges = composite_edges.clone();
    let cross_repo_edges = resolve_cross_repo_edges(&a, &b, &moniker_ids);
    let cross_repo_count = cross_repo_edges.len();
    composite_edges.extend(cross_repo_edges.iter().copied());

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

    // Record each repo's boundary contract as the other repo's
    // expectation, so `weave check-contracts` can detect divergence later.
    // The per-symbol map is recorded too, not just its hash, so a later
    // divergence can be diffed symbol-by-symbol.
    let map_a = repo_contract_map(&a, repo_a)?;
    let map_b = repo_contract_map(&b, repo_b)?;
    let hash_a = crate::contracts::contract_hash_of(&map_a);
    let hash_b = crate::contracts::contract_hash_of(&map_b);
    crate::contracts::record_expectations(repo_a, repo_b, &map_a, &map_b, None, None)?;
    println!(
        "Recorded contract expectations: {} = {}, {} = {}",
        a.label, hash_a, b.label, hash_b
    );

    // Auto-append `repo_b` to `repo_a`'s config so `weave link` works seamlessly later.
    let config_a = repo_a.join(".weave").join("config.toml");
    crate::config::add_linked_repo(&config_a, repo_b)?;
    let config_b = repo_b.join(".weave").join("config.toml");
    crate::config::add_linked_repo(&config_b, repo_a)?;

    // The composite graph used to be built, reported, and thrown away —
    // `weave query-federated` needs it to still exist
    // after this process exits. Persisted symmetrically so either side can
    // query without caring which repo actually ran `weave link`.
    persist_composite_graph(
        repo_a,
        &b.label,
        &composite_nodes,
        &local_edges,
        &cross_repo_edges,
    )?;
    persist_composite_graph(
        repo_b,
        &a.label,
        &composite_nodes,
        &local_edges,
        &cross_repo_edges,
    )?;
    println!(
        "Persisted federated graph ({} <-> {}); query it with `weave query-federated`.",
        a.label, b.label
    );

    Ok(())
}

/// Writes the composite graph to `.weave/federation/<partner_label>.db` so
/// `weave query-federated` can reload it later — the same crash-safe
/// stage-then-rename `weave index` already uses for `graph.db`
/// (`AGENTS.md` Invariant #2).
fn persist_composite_graph(
    repo_dir: &Path,
    partner_label: &str,
    nodes: &[Node],
    local_edges: &[(u32, u32)],
    cross_repo_edges: &[(u32, u32)],
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = repo_dir.join(".weave").join("federation");
    fs::create_dir_all(&dir)?;
    let active_db = dir.join(format!("{partner_label}.db"));
    let rebuild_db = dir.join(format!("{partner_label}.db.rebuild"));
    if rebuild_db.exists() {
        fs::remove_file(&rebuild_db)?;
    }
    let mut storage = SqliteStorage::open(&rebuild_db)?;
    storage.begin_bulk_write()?;
    // `upsert_node` always assigns its own autoincrement id (it ignores
    // `Node::id` entirely), so the edges below — built against
    // composite-vector positions — must be translated through this map.
    let mut db_id = Vec::with_capacity(nodes.len());
    for node in nodes {
        db_id.push(storage.upsert_node(node)?);
    }
    for &(src, tgt) in local_edges {
        storage.upsert_edge(&Edge {
            id: 0,
            source_id: db_id[src as usize],
            target_id: db_id[tgt as usize],
            kind: "LOCAL".to_string(),
            weight: 1.0,
        })?;
    }
    for &(src, tgt) in cross_repo_edges {
        storage.upsert_edge(&Edge {
            id: 0,
            source_id: db_id[src as usize],
            target_id: db_id[tgt as usize],
            kind: "CROSS_REPO".to_string(),
            weight: 1.0,
        })?;
    }
    storage.commit_bulk_write()?;
    storage.checkpoint_wal()?;
    drop(storage);
    fs::rename(&rebuild_db, &active_db)?;
    Ok(())
}

/// Locates and opens the composite graph `weave link` persisted for this
/// pair (`persist_composite_graph`) — shared by every federated-graph
/// reader (`query-federated`, `report-federated`) so "no federated graph
/// yet" always produces the same clear error naming the fix.
pub(crate) fn open_federated_storage(
    repo_a: &Path,
    repo_b: &Path,
) -> Result<(SqliteStorage, PathBuf), Box<dyn std::error::Error>> {
    let partner_label = repo_label(repo_b);
    let db_path = repo_a
        .join(".weave")
        .join("federation")
        .join(format!("{partner_label}.db"));
    if !db_path.exists() {
        return Err(format!(
            "No federated graph at {} — run `weave link {} {}` first.",
            db_path.display(),
            repo_a.display(),
            repo_b.display()
        )
        .into());
    }
    let storage = SqliteStorage::open_read_only(&db_path)?;
    Ok((storage, db_path))
}

pub(crate) fn cmd_query_federated(
    repo_a: &Path,
    repo_b: &Path,
    expression: &str,
    as_subject: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (storage, _db_path) = open_federated_storage(repo_a, repo_b)?;
    #[cfg(feature = "rbac")]
    let guard = as_subject.map(|subject| FederatedRbac::new(repo_a, repo_b, subject));
    #[cfg(feature = "rbac")]
    let masker = |node: &Node| {
        guard
            .as_ref()
            .map_or_else(|| node.clone(), |g| g.mask(node))
    };
    #[cfg(feature = "rbac")]
    let mask = guard.as_ref().map(|_| &masker as &dyn Fn(&Node) -> Node);
    #[cfg(not(feature = "rbac"))]
    let (mask, _) = (None::<&dyn Fn(&Node) -> Node>, as_subject);
    match crate::query::run(&storage, expression, mask) {
        Ok(text) => {
            println!("{text}");
            Ok(())
        }
        Err(message) => Err(message.into()),
    }
}

/// Uses each repository's guard because a composite graph spans two
/// independently configured authorization domains.
pub(crate) fn cmd_report_federated(
    repo_a: &Path,
    repo_b: &Path,
    out_dir: Option<&Path>,
    as_subject: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (storage, db_path) = open_federated_storage(repo_a, repo_b)?;
    let partner_label = repo_label(repo_b);
    let out_dir = out_dir.map(Path::to_path_buf).unwrap_or_else(|| {
        repo_a
            .join(".weave")
            .join("federation-report")
            .join(&partner_label)
    });
    #[cfg(feature = "rbac")]
    let guard = as_subject.map(|subject| FederatedRbac::new(repo_a, repo_b, subject));
    #[cfg(feature = "rbac")]
    let visible_check = |node: &Node| guard.as_ref().is_none_or(|g| g.visible(node));
    #[cfg(feature = "rbac")]
    let visible = guard
        .as_ref()
        .map(|_| &visible_check as &dyn Fn(&Node) -> bool);
    #[cfg(not(feature = "rbac"))]
    let (visible, _) = (None::<&dyn Fn(&Node) -> bool>, as_subject);
    let paths = crate::report::generate(repo_a, &out_dir, &db_path, &storage, None, visible)?;
    println!("Wrote {}", paths.report_md.display());
    for canvas in &paths.canvas_files {
        println!("Wrote {}", canvas.display());
    }
    Ok(())
}

/// `weave link` with fewer than two explicit paths: the partner comes
/// from this repo's `[federation] linked_repos` config.
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

/// Whole-repo exported-symbol map from already-parsed sources: every
/// file's exported symbols, keyed by qualified name. Reuses the
/// `parse_all` output `load_repo` already produced — no
/// second file read. Paths are relativized against `root` so this agrees
/// with `contracts::repo_contract_map`'s own relative-path convention —
/// otherwise the two sides of a later diff would show the same untouched
/// symbol under two different path spellings.
fn repo_contract_map(
    graph: &RepoGraph,
    root: &Path,
) -> Result<crate::contracts::ContractMap, Box<dyn std::error::Error>> {
    let mut map = crate::contracts::ContractMap::new();
    for (path, parsed) in &graph.parsed_files {
        // `from_path` dispatches on extension, so the absolute path
        // `parse_all` produced needs no rewriting here.
        let Some(language) = Language::from_path(path) else {
            continue;
        };
        let rel = path.strip_prefix(root).unwrap_or(path);
        let path_str = rel.to_string_lossy();
        for (symbol, (kind, signature, line_start)) in
            contract::exported_entries_map(language, parsed)
        {
            map.insert(symbol, (kind, signature, path_str.to_string(), line_start));
        }
    }
    Ok(map)
}

#[cfg(test)]
fn repo_contract_hash(
    graph: &RepoGraph,
    root: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(crate::contracts::contract_hash_of(&repo_contract_map(
        graph, root,
    )?))
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
