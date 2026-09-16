//! `weave plan-migration` walks the
//! federated call graph for every cross-repo caller of a deprecated
//! symbol, orders the per-repo changes by dependency (callers migrate
//! before the repo providing the symbol can remove it), and emits the
//! plan as a structured document — never an automated code-mod.
//!
//! The ordering graph is the *full* cross-repo dependency graph among the
//! affected repos, not just the deprecated symbol's caller edges — a
//! circular dependency anywhere in the affected set would make any linear
//! plan a lie, so cycles are detected (Tarjan's SCC) and reported instead
//! of silently ordered arbitrarily.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use weave_graph_core::federation::tarjan_scc;

use crate::federation::{RepoGraph, load_repo};

/// One affected repo: which repo it depends on for the deprecated symbol,
/// and which of its own files hold the callers.
pub(crate) struct AffectedRepo {
    label: String,
    caller_files: Vec<String>,
}

/// The survey result: the provider repo's label, the affected caller
/// repos, and the migration order (`None` = cyclic, refuse to order).
/// `node_labels`/`ordering_edges` carry the same repo graph the order was
/// computed from, so the cycle report names exactly what blocked it.
pub(crate) struct Survey {
    pub(crate) provider: String,
    pub(crate) affected: Vec<AffectedRepo>,
    pub(crate) order: Option<Vec<String>>,
    pub(crate) node_labels: Vec<String>,
    pub(crate) ordering_edges: Vec<(u32, u32)>,
}

/// `path#symbol` — the moniker's two halves.
fn split_moniker(m: &str) -> (&str, &str) {
    match m.split_once('#') {
        Some((path, sym)) => (path, sym),
        None => ("", m),
    }
}

/// One resolved cross-repo reference between two repos.
struct CrossRef {
    caller: String,
    target: String,
    source_file: String,
    target_symbol: String,
}

/// Re-parses every repo's unresolved references against every other
/// repo's index — the N-repo generalization of the two-repo pairwise
/// resolution `cmd_link` does.
fn cross_references(repos: &[RepoGraph]) -> Vec<CrossRef> {
    let mut refs = Vec::new();
    for (r, caller) in repos.iter().enumerate() {
        for (p, provider) in repos.iter().enumerate() {
            if r == p {
                continue;
            }
            for (path, parsed) in &caller.parsed_files {
                for edge in caller
                    .project_index
                    .resolve_cross_repo(parsed, &provider.project_index)
                {
                    refs.push(CrossRef {
                        caller: caller.label.clone(),
                        target: provider.label.clone(),
                        source_file: path.to_string_lossy().to_string(),
                        target_symbol: split_moniker(&edge.target_moniker).1.to_string(),
                    });
                }
            }
        }
    }
    refs
}

pub(crate) fn survey(
    repos: &[RepoGraph],
    symbol: &str,
) -> Result<Survey, Box<dyn std::error::Error>> {
    let provider = repos
        .iter()
        .find(|g| g.nodes.iter().any(|n| n.symbol == symbol))
        .ok_or_else(|| format!("no linked repo defines a symbol named {symbol:?}"))?;
    let provider_label = provider.label.clone();

    let refs = cross_references(repos);
    let mut affected: BTreeMap<String, AffectedRepo> = BTreeMap::new();
    for r in &refs {
        if r.target != provider_label || r.target_symbol != symbol || r.caller == provider_label {
            continue;
        }
        affected
            .entry(r.caller.clone())
            .or_insert_with(|| AffectedRepo {
                label: r.caller.clone(),
                caller_files: Vec::new(),
            })
            .caller_files
            .push(r.source_file.clone());
    }
    for repo in affected.values_mut() {
        repo.caller_files.sort();
        repo.caller_files.dedup();
    }
    if affected.is_empty() {
        return Err(format!(
            "no cross-repo callers of {symbol:?} found in the linked repos — nothing to plan"
        )
        .into());
    }

    // The ordering graph: every cross-repo reference whose endpoints are
    // both in {affected repos, provider}. Repo A -> repo B means A
    // depends on B: A migrates before B.
    let mut nodes: BTreeSet<&str> = affected.keys().map(|s| s.as_str()).collect();
    nodes.insert(provider_label.as_str());
    let mut edges: Vec<(u32, u32)> = Vec::new();
    let mut ids: HashMap<&str, u32> = HashMap::new();
    for (i, label) in nodes.iter().enumerate() {
        ids.insert(*label, i as u32);
    }
    for r in &refs {
        if nodes.contains(r.caller.as_str()) && nodes.contains(r.target.as_str()) {
            edges.push((ids[r.caller.as_str()], ids[r.target.as_str()]));
        }
    }

    let node_labels: Vec<String> = nodes.iter().map(|l| l.to_string()).collect();
    let order = kahn(nodes.len(), &edges).map(|seq| {
        let by_id: Vec<&str> = nodes.iter().copied().collect();
        seq.into_iter()
            .map(|i| by_id[i as usize].to_string())
            .collect()
    });
    Ok(Survey {
        provider: provider_label,
        affected: affected.into_values().collect(),
        order,
        node_labels,
        ordering_edges: edges,
    })
}

/// Kahn's algorithm. `None` = the remaining nodes are cyclic; the plan
/// reports the cycle rather than guessing an order.
fn kahn(node_count: usize, edges: &[(u32, u32)]) -> Option<Vec<u32>> {
    let mut indegree = vec![0usize; node_count];
    let mut dependents: Vec<Vec<u32>> = vec![Vec::new(); node_count];
    for &(from, to) in edges {
        indegree[to as usize] += 1;
        dependents[from as usize].push(to);
    }
    let mut ready: Vec<u32> = (0..node_count as u32)
        .filter(|n| indegree[*n as usize] == 0)
        .collect();
    let mut order = Vec::new();
    while let Some(next) = ready.pop() {
        order.push(next);
        for &dep in &dependents[next as usize] {
            indegree[dep as usize] -= 1;
            if indegree[dep as usize] == 0 {
                ready.push(dep);
            }
        }
    }
    (order.len() == node_count).then_some(order)
}

fn render_plan(
    provider: &str,
    symbol: &str,
    affected: &[AffectedRepo],
    order: &[String],
) -> String {
    let mut plan = format!(
        "# Migration Plan: deprecate `{symbol}` ({provider})\n\n\
         {n} repo(s) call it across the federation. Order is dependency-driven: a repo\n\
         that depends on another migrates first, so the deprecated symbol is only\n\
         removed after every caller is off it.\n\n",
        n = affected.len()
    );
    for (step, label) in order.iter().enumerate() {
        plan.push_str(&format!("## Step {}: {label}\n", step + 1));
        if label == provider {
            plan.push_str(&format!(
                "- Remove or rename `{symbol}` now that every listed caller has migrated.\n"
            ));
            continue;
        }
        let Some(repo) = affected.iter().find(|r| &r.label == label) else {
            plan.push_str("- Caller inventory was unavailable; re-run the migration survey.\n");
            continue;
        };
        plan.push_str(&format!(
            "- Update callers of `{symbol}` (provided by {provider}) in:\n"
        ));
        for file in &repo.caller_files {
            plan.push_str(&format!("  - {file}\n"));
        }
    }
    plan
}

fn cycle_message(survey: &Survey) -> String {
    let cycles: Vec<Vec<u32>> = tarjan_scc(
        &(0..survey.node_labels.len() as u32).collect::<Vec<u32>>(),
        &survey.ordering_edges,
    )
    .into_iter()
    .filter(|c| c.len() > 1)
    .collect();
    let names: Vec<String> = cycles
        .iter()
        .map(|c| {
            c.iter()
                .map(|i| survey.node_labels[*i as usize].as_str())
                .collect::<Vec<_>>()
                .join(" -> ")
        })
        .collect();
    format!(
        "cross-repo dependency cycle detected ({}) — refusing to pick an arbitrary \
         migration order; break the cycle first",
        names.join(" | ")
    )
}

/// `weave plan-migration --symbol <name>`: the own repo plus every
/// `[federation] linked_repos` entry forms the surveyed federation.
pub(crate) fn cmd_plan_migration(
    root: &Path,
    symbol: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = root.join(".weave").join("config.toml");
    let mut repo_roots: Vec<PathBuf> = vec![root.to_path_buf()];
    repo_roots.extend(
        crate::config::read_linked_repos(&config_path)
            .into_iter()
            .map(|p| if p.is_absolute() { p } else { root.join(p) }),
    );

    let mut repos = Vec::new();
    for repo_root in &repo_roots {
        repos.push(load_repo(repo_root)?);
    }
    let labels: BTreeSet<&str> = repos.iter().map(|r| r.label.as_str()).collect();
    if labels.len() != repos.len() {
        return Err(
            "two configured repos resolve to the same label — federation needs distinguishable repos"
                .into(),
        );
    }

    let result = survey(&repos, symbol)?;
    let Some(order) = result.order else {
        return Err(cycle_message(&result).into());
    };

    let plan = render_plan(&result.provider, symbol, &result.affected, &order);
    println!("{plan}");
    let plan_path = root.join(".weave").join("MIGRATION_PLAN.md");
    if plan_path.parent().is_some_and(Path::exists) {
        fs::write(&plan_path, &plan)?;
        println!("Written to {}", plan_path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
