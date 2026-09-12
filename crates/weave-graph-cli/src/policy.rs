//! `weave policy` (`impl.md` M3.2, `plan.md` §3.2): YAML-declared
//! architectural boundaries evaluated against the indexed graph — the CI
//! gate is simply this command's exit code — plus drift analytics
//! (dependency cycles, orphaned files). YAML parsing lives here; the
//! rule model and evaluation stay in `weave_graph_core::policy`, which
//! never sees a file path.

use std::path::Path;

use serde::Deserialize;
use weave_graph_core::policy::{Boundary, BoundaryRule};
use weave_graph_core::{Edge, Node};

const POLICY_FILE: &str = ".weave/policy.yaml";

#[derive(Deserialize)]
struct PolicyFile {
    #[serde(default)]
    rules: Vec<RuleEntry>,
}

#[derive(Deserialize)]
struct RuleEntry {
    disallow: Option<BoundaryYaml>,
    require: Option<BoundaryYaml>,
}

#[derive(Deserialize)]
struct BoundaryYaml {
    from: String,
    to: String,
}

fn to_boundary(b: &BoundaryYaml) -> Boundary {
    Boundary {
        from: b.from.clone(),
        to: b.to.clone(),
    }
}

/// Parses and validates `.weave/policy.yaml` into core rule values. A
/// rule with both actions, neither action, or an empty endpoint is a
/// config error, not a lint failure — refuse loudly rather than lint a
/// policy that doesn't say what its author thought it did.
pub(crate) fn load_rules(path: &Path) -> Result<Vec<BoundaryRule>, String> {
    let content = std::fs::read_to_string(path).map_err(|_| {
        format!(
            "policy file not found: {} — create it to declare boundaries",
            path.display()
        )
    })?;
    let parsed: PolicyFile =
        serde_yaml::from_str(&content).map_err(|e| format!("invalid policy YAML: {e}"))?;
    let mut rules = Vec::new();
    for entry in &parsed.rules {
        let disallow = entry.disallow.as_ref().map(to_boundary);
        let require = entry.require.as_ref().map(to_boundary);
        let rule = match (disallow, require) {
            (Some(_), Some(_)) => {
                return Err("a rule cannot be both `disallow` and `require`".to_string());
            }
            (Some(boundary), None) => {
                validate_boundary(&boundary)?;
                BoundaryRule::Disallow(boundary)
            }
            (None, Some(boundary)) => {
                validate_boundary(&boundary)?;
                BoundaryRule::Require(boundary)
            }
            (None, None) => {
                return Err("each rule needs exactly one of `disallow` or `require`".to_string());
            }
        };
        rules.push(rule);
    }
    Ok(rules)
}

fn validate_boundary(boundary: &Boundary) -> Result<(), String> {
    if boundary.from.is_empty() || boundary.to.is_empty() {
        return Err("boundary endpoints must be non-empty path prefixes".to_string());
    }
    Ok(())
}
/// `weave policy lint`: violations on stdout, non-zero exit on any. With
/// `rbac` + `--as`, the linted view is the masked one — edges touching
/// hidden symbols cannot be classified and are reported as skipped, never
/// silently dropped and never invented into violations.
pub(crate) fn cmd_policy_lint(
    root: &Path,
    as_subject: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (storage, _db) = crate::open_storage_for_read(root)?;
    let rules = load_rules(&root.join(POLICY_FILE))?;

    #[cfg(feature = "rbac")]
    let guard = as_subject.map(|s| crate::rbac::guard_for(root, Some(s)));
    #[cfg(feature = "rbac")]
    let visible_check = guard.as_ref().map(|g| |n: &Node| g.visible(n));
    #[cfg(feature = "rbac")]
    let visible: Option<&dyn Fn(&Node) -> bool> =
        visible_check.as_ref().map(|c| c as &dyn Fn(&Node) -> bool);
    #[cfg(not(feature = "rbac"))]
    let (visible, _) = (None::<&dyn Fn(&Node) -> bool>, as_subject);

    let view = visible_view(&storage, visible)?;
    let violations = weave_graph_core::policy::lint(&view.nodes, &view.edges, &rules);

    println!("Policy: {} rule(s) from {}", rules.len(), POLICY_FILE);
    if view.hidden_nodes > 0 || view.skipped_edges > 0 {
        println!(
            "  {} edge(s) and {} symbol(s) skipped (rbac-masked)",
            view.skipped_edges, view.hidden_nodes
        );
    }
    if violations.is_empty() {
        println!("✓ no boundary violations");
    } else {
        for v in &violations {
            println!("✗ [{}] {} -> {}", v.kind, v.from, v.to);
            for example in &v.examples {
                println!("    {example}");
            }
        }
    }

    #[cfg(feature = "slm")]
    print_adr_obligations(root);

    if !violations.is_empty() {
        return Err(format!(
            "{} policy violation(s) — blocking (CI gate)",
            violations.len()
        )
        .into());
    }
    Ok(())
}

/// `weave policy drift`: advisory architecture-rot report — dependency
/// cycles and files nothing depends on. Always exits 0; these are
/// findings for a human, not a gate.
pub(crate) fn cmd_policy_drift(
    root: &Path,
    as_subject: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (storage, _db) = crate::open_storage_for_read(root)?;

    #[cfg(feature = "rbac")]
    let guard = as_subject.map(|s| crate::rbac::guard_for(root, Some(s)));
    #[cfg(feature = "rbac")]
    let visible_check = guard.as_ref().map(|g| |n: &Node| g.visible(n));
    #[cfg(feature = "rbac")]
    let visible: Option<&dyn Fn(&Node) -> bool> =
        visible_check.as_ref().map(|c| c as &dyn Fn(&Node) -> bool);
    #[cfg(not(feature = "rbac"))]
    let (visible, _) = (None::<&dyn Fn(&Node) -> bool>, as_subject);

    let view = visible_view(&storage, visible)?;
    let cycles = weave_graph_core::policy::find_cycles(&view.nodes, &view.edges);
    let orphans = weave_graph_core::policy::orphan_files(&view.nodes, &view.edges);

    println!("Drift report:");
    if cycles.is_empty() {
        println!("  no dependency cycles");
    } else {
        println!("  {} dependency cycle(s):", cycles.len());
        for cycle in &cycles {
            println!("    {}", cycle.join(" -> "));
        }
    }
    if orphans.is_empty() {
        println!("  no orphaned files (every file has an inbound dependency)");
    } else {
        println!(
            "  {} orphaned file(s) (no inbound cross-file dependency):",
            orphans.len()
        );
        for file in &orphans {
            println!("    {file}");
        }
    }
    Ok(())
}

/// The graph view lint/drift actually evaluate: nodes filtered by the
/// caller's visibility predicate, edges kept only when both endpoints
/// survived. Cross-module classification needs both sides; a half-visible
/// edge is unclassifiable and counted, never guessed about.
struct GraphView {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    hidden_nodes: usize,
    skipped_edges: usize,
}

fn visible_view(
    storage: &dyn weave_graph_core::Storage,
    visible: Option<&dyn Fn(&Node) -> bool>,
) -> Result<GraphView, Box<dyn std::error::Error>> {
    let all_nodes = storage.all_nodes()?;
    let (nodes, hidden_nodes) = match visible {
        Some(check) => {
            let mut nodes = Vec::new();
            let mut hidden = 0usize;
            for node in all_nodes {
                if check(&node) {
                    nodes.push(node);
                } else {
                    hidden += 1;
                }
            }
            (nodes, hidden)
        }
        None => (all_nodes, 0),
    };
    let visible_ids: std::collections::HashSet<u32> = nodes.iter().map(|n| n.id).collect();
    let all_edges = storage.all_edges()?;
    let mut edges = Vec::new();
    let mut skipped_edges = 0usize;
    for edge in all_edges {
        if visible_ids.contains(&edge.source_id) && visible_ids.contains(&edge.target_id) {
            edges.push(edge);
        } else {
            skipped_edges += 1;
        }
    }
    Ok(GraphView {
        nodes,
        edges,
        hidden_nodes,
        skipped_edges,
    })
}

/// The M2.4.4 composition (`impl.md` M3.2): confirmed ADR obligations are
/// advisory context next to the machine-checked rules. Prose like
/// "services must not call the database directly" has no mechanical
/// `from`/`to` mapping a linter could enforce without guessing — surfacing
/// it as informational, not blocking, is the honest seam between the two
/// features.
#[cfg(feature = "slm")]
fn print_adr_obligations(root: &Path) {
    let state = crate::rules::load_state(&crate::rules::rules_file(root));
    if state.confirmed.is_empty() {
        return;
    }
    println!(
        "\nADR obligations confirmed via `weave slm review-rules` (informational, not enforced):"
    );
    for rule in &state.confirmed {
        println!("  - {:?} ({}:{})", rule.text, rule.file, rule.line);
    }
}

#[cfg(test)]
mod tests;
