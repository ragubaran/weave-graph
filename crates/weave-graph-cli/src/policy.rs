//! `weave policy`: YAML-declared
//! architectural boundaries evaluated against the indexed graph — the CI
//! gate is simply this command's exit code — plus drift analytics
//! (dependency cycles, orphaned files). YAML parsing lives here; the
//! rule model and evaluation stay in `weave_graph_core::policy`, which
//! never sees a file path.

use std::path::Path;

use serde::Deserialize;
use weave_graph_core::policy::{Boundary, BoundaryRule, Violation};
use weave_graph_core::{Edge, Node};

const POLICY_FILE: &str = ".weave/policy.yaml";

#[derive(Deserialize)]
struct PolicyFile {
    #[serde(default)]
    rules: Vec<RuleEntry>,
    /// POL-02: advisory-only, `weave policy drift` alone reads this — see
    /// `load_semantic_coupling_rules`.
    #[cfg(feature = "vector")]
    #[serde(default)]
    semantic_coupling: Vec<SemanticCouplingYaml>,
}

#[cfg(feature = "vector")]
#[derive(Deserialize)]
struct SemanticCouplingYaml {
    within: String,
    threshold: f32,
    #[serde(default)]
    exempt_globs: Vec<String>,
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
    /// POL-04: roles exempt from this rule.
    #[serde(default)]
    allowed_roles: Vec<String>,
    /// POL-04: reporting-only team attribution.
    #[serde(default)]
    owner_role: Option<String>,
}

fn to_boundary(b: &BoundaryYaml) -> Boundary {
    Boundary {
        from: b.from.clone(),
        to: b.to.clone(),
        allowed_roles: b.allowed_roles.clone(),
        owner_role: b.owner_role.clone(),
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

/// POL-02: parses the optional `semantic_coupling` list from the same
/// policy file `load_rules` reads. A separate, always-succeeds-on-absence
/// pass (unlike `load_rules`) since only the advisory `weave policy drift`
/// consults it — an unwritten policy file is a normal drift run, not a
/// config error.
#[cfg(feature = "vector")]
pub(crate) fn load_semantic_coupling_rules(
    path: &Path,
) -> Result<Vec<weave_graph_core::policy::SemanticCouplingRule>, String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Ok(Vec::new());
    };
    let parsed: PolicyFile =
        serde_yaml::from_str(&content).map_err(|e| format!("invalid policy YAML: {e}"))?;
    let mut rules = Vec::new();
    for entry in parsed.semantic_coupling {
        if entry.within.is_empty() {
            return Err("semantic_coupling rule needs a non-empty `within` prefix".to_string());
        }
        if !(0.0..=1.0).contains(&entry.threshold) {
            return Err("semantic_coupling `threshold` must be between 0.0 and 1.0".to_string());
        }
        rules.push(weave_graph_core::policy::SemanticCouplingRule {
            within: entry.within,
            threshold: entry.threshold,
            exempt_globs: entry.exempt_globs,
        });
    }
    Ok(rules)
}
/// A violation's stable rule id for `--waive`: `<kind>:<from>-><to>`,
/// exactly what the unwaived print path already renders — no separate ID
/// scheme to keep in sync.
fn rule_id(v: &Violation) -> String {
    format!("{}:{}->{}", v.kind, v.from, v.to)
}

/// POL-05: appends one append-only line to `.weave/policy-waivers.log`
/// per waived rule — `timestamp, subject, rule-id, reason`, plain text,
/// no rotation logic. Never overwrites; a missing file is created.
fn append_waiver_log(
    root: &Path,
    as_subject: Option<&str>,
    rule_id: &str,
    reason: &str,
) -> std::io::Result<()> {
    use std::io::Write;
    let path = root.join(".weave").join("policy-waivers.log");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let timestamp = crate::provenance::format_utc(std::time::SystemTime::now());
    let subject = as_subject.unwrap_or("anonymous");
    writeln!(file, "{timestamp}, {subject}, {rule_id}, {reason}")
}

/// Shared by the single-repo and FED-01 federated paths: waives, prints,
/// logs, and turns the final verdict into the command's `Result`. Neither
/// path duplicates this — only how `violations`/`hidden_nodes`/
/// `skipped_edges` get computed differs between them.
fn report_and_gate(
    root: &Path,
    gate: LintGate,
    rule_count: usize,
    violations: Vec<Violation>,
    hidden_nodes: usize,
    skipped_edges: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    // POL-05: waiving requires the same audit-trail gate every other
    // bypass in this codebase uses — authorized before any violation is
    // actually exempted, never after the fact.
    let waive_reason = if gate.waive.is_empty() {
        None
    } else {
        crate::waiver::authorize(root, gate.as_subject)?;
        Some(crate::waiver::require_reason(gate.reason)?)
    };
    let waive_ids: std::collections::HashSet<&str> =
        gate.waive.iter().map(String::as_str).collect();
    let (waived, blocking): (Vec<Violation>, Vec<Violation>) = violations
        .into_iter()
        .partition(|v| waive_ids.contains(rule_id(v).as_str()));

    println!("Policy: {rule_count} rule(s) from {POLICY_FILE}");
    if hidden_nodes > 0 || skipped_edges > 0 {
        println!("  {skipped_edges} edge(s) and {hidden_nodes} symbol(s) skipped (rbac-masked)");
    }
    let incomplete = hidden_nodes > 0 || skipped_edges > 0;
    if blocking.is_empty() && waived.is_empty() && !incomplete {
        println!("✓ no boundary violations");
    }
    for v in &waived {
        println!("⚠ [{}] {} -> {} — WAIVED", v.kind, v.from, v.to);
        for example in &v.examples {
            println!("    {example}");
        }
    }
    for v in &blocking {
        match &v.owner_role {
            Some(owner) => println!("✗ [{}] {} -> {} (owner: {owner})", v.kind, v.from, v.to),
            None => println!("✗ [{}] {} -> {}", v.kind, v.from, v.to),
        }
        for example in &v.examples {
            println!("    {example}");
        }
    }

    if let Some(reason) = &waive_reason {
        print!(
            "{}",
            crate::waiver::emit_banner("weave policy lint", reason)
        );
        for v in &waived {
            append_waiver_log(root, gate.as_subject, &rule_id(v), reason)?;
        }
    }

    #[cfg(feature = "slm")]
    print_adr_obligations(root);

    if !blocking.is_empty() {
        return Err(format!(
            "{} policy violation(s) — blocking (CI gate)",
            blocking.len()
        )
        .into());
    }

    if incomplete && (gate.fail_on_masked || gate.as_subject.is_some()) {
        return Err(
            "Policy view incomplete: nodes or edges were skipped by RBAC, so this run cannot certify repository-wide boundaries."
            .into()
        );
    }
    Ok(())
}

/// Gate settings shared by both `cmd_policy_lint`'s local and federated
/// paths, bundled for the same reason `contracts::CheckContractsWaiver`
/// bundles its own flags: `report_and_gate` never touches `std::env`
/// itself, so the caller (`main.rs`) collects these once, from the CLI
/// flags plus `--as`.
struct LintGate<'a> {
    as_subject: Option<&'a str>,
    fail_on_masked: bool,
    waive: &'a [String],
    reason: Option<&'a str>,
}

/// FED-01: `Boundary{from, to}` prefixes matched across every
/// `[federation] linked_repos` peer instead of within one repo — reusing
/// `federation::open_federated_storage`'s existing per-peer graph exactly
/// as `weave check-contracts --scoped` already does, not a new plumbing
/// path. Linted **per peer, separately** (never merged into one shared
/// node-id space): each peer's federated database is its own independent
/// `SqliteStorage` with its own id sequence, so a merged `Vec<Node>` would
/// let two unrelated nodes from different peers collide on the same id.
/// Violations are deduped by `(kind, from, to)` across peers.
#[cfg(feature = "federation")]
fn cmd_policy_lint_federated(
    root: &Path,
    gate: LintGate,
    rules: &[BoundaryRule],
    visible: Option<&dyn Fn(&Node) -> bool>,
    current_roles: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = root.join(".weave").join("config.toml");
    let linked: Vec<std::path::PathBuf> = crate::config::read_linked_repos(&config_path)
        .into_iter()
        .map(|p| if p.is_absolute() { p } else { root.join(p) })
        .collect();
    if linked.is_empty() {
        return Err(
            "No linked repos in .weave/config.toml ([federation] linked_repos). \
             Run `weave link <repo-a> <repo-b>` first."
                .into(),
        );
    }

    let mut seen = std::collections::HashSet::new();
    let mut violations = Vec::new();
    let mut hidden_nodes = 0usize;
    let mut skipped_edges = 0usize;
    for peer in &linked {
        let (storage, _) = crate::federation::open_federated_storage(root, peer)?;
        let (nodes, edges) = fetch_graph(&storage)?;
        let view = filter_view(nodes, edges, visible);
        hidden_nodes += view.hidden_nodes;
        skipped_edges += view.skipped_edges;
        for v in
            weave_graph_core::policy::lint_scoped(&view.nodes, &view.edges, rules, current_roles)
        {
            if seen.insert((v.kind, v.from.clone(), v.to.clone())) {
                violations.push(v);
            }
        }
    }

    report_and_gate(
        root,
        gate,
        rules.len(),
        violations,
        hidden_nodes,
        skipped_edges,
    )
}

/// `weave policy lint`: violations on stdout, non-zero exit on any. With
/// `rbac` + `--as`, the linted view is the masked one — edges touching
/// hidden symbols cannot be classified and are reported as skipped, never
/// silently dropped and never invented into violations.
///
/// `waive` names rule ids (POL-05, [`rule_id`]'s format) to exempt from
/// blocking — never from the printed report, so a waived violation is
/// still visible, just not fatal. Requires `--reason` and the same
/// `allow-drift` gate `weave check-contracts`/`weave blast` already use
/// (`waiver::authorize`); each waived id is appended to
/// `.weave/policy-waivers.log`.
///
/// `federated` (FED-01) lints across every linked repo instead of just
/// `root` — see [`cmd_policy_lint_federated`].
pub(crate) fn cmd_policy_lint(
    root: &Path,
    as_subject: Option<&str>,
    fail_on_masked: bool,
    waive: &[String],
    reason: Option<&str>,
    federated: bool,
) -> Result<(), Box<dyn std::error::Error>> {
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
    // POL-04: an unauthenticated run (no `--as`, or `rbac` not compiled
    // in) carries no roles, so `allowed_roles` exemptions never apply —
    // matches `lint()`'s own pre-POL-04 behavior exactly.
    #[cfg(feature = "rbac")]
    let current_roles: Vec<String> = guard
        .as_ref()
        .map(|g| g.roles().to_vec())
        .unwrap_or_default();
    #[cfg(not(feature = "rbac"))]
    let current_roles: Vec<String> = Vec::new();

    let gate = LintGate {
        as_subject,
        fail_on_masked,
        waive,
        reason,
    };

    if federated {
        #[cfg(feature = "federation")]
        {
            return cmd_policy_lint_federated(root, gate, &rules, visible, &current_roles);
        }
        #[cfg(not(feature = "federation"))]
        {
            let _ = gate;
            return Err(
                "weave policy lint --federated requires the `federation` feature, which is \
                 not compiled into this binary."
                    .into(),
            );
        }
    }

    let (storage, _db) = crate::open_storage_for_read(root)?;
    let (all_nodes, all_edges) = fetch_graph(&storage)?;
    let view = filter_view(all_nodes, all_edges, visible);
    let violations =
        weave_graph_core::policy::lint_scoped(&view.nodes, &view.edges, &rules, &current_roles);

    report_and_gate(
        root,
        gate,
        rules.len(),
        violations,
        view.hidden_nodes,
        view.skipped_edges,
    )
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

    let (all_nodes, all_edges) = fetch_graph(&storage)?;
    // POL-03: a masked view can sever a public file's only inbound edges
    // (they came from a hidden module), making it falsely look orphaned.
    // Computed from the same already-fetched `all_nodes`/`all_edges` — one
    // `Storage` round-trip total, not two, and no second full node/edge
    // `Vec` held alongside the masked one (Invariant 4's RAM envelope).
    let unmasked_orphans = visible.is_some().then(|| {
        weave_graph_core::policy::orphan_files(&all_nodes, &all_edges)
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
    });

    let view = filter_view(all_nodes, all_edges, visible);
    let cycles = weave_graph_core::policy::find_cycles(&view.nodes, &view.edges);
    let orphans = weave_graph_core::policy::orphan_files(&view.nodes, &view.edges);
    let truly_orphaned = unmasked_orphans.unwrap_or_else(|| orphans.iter().cloned().collect());

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
            if truly_orphaned.contains(file) {
                println!("    {file}");
            } else {
                println!("    {file} (has hidden inbound edges)");
            }
        }
    }
    #[cfg(feature = "vector")]
    {
        let findings = semantic_coupling_report(root, &storage, &view.nodes, &view.edges)?;
        if findings.is_empty() {
            println!("  no undeclared semantic coupling above the configured threshold(s)");
        } else {
            println!(
                "  {} undeclared semantic coupling pair(s) (advisory, POL-02):",
                findings.len()
            );
            for finding in &findings {
                println!(
                    "    {} <-> {} (similarity {:.2})",
                    finding.file_a, finding.file_b, finding.similarity
                );
            }
        }
    }
    Ok(())
}

/// POL-02: high-similarity file pairs with no declared edge between them.
/// Opt-in via `.weave/policy.yaml`'s `semantic_coupling` list — most repos
/// never set it, so an absent list means zero rules and an empty report,
/// not an error. Split from its printing so tests can assert on the
/// findings directly rather than scraping stdout.
#[cfg(feature = "vector")]
pub(crate) fn semantic_coupling_report(
    root: &Path,
    storage: &dyn weave_graph_core::Storage,
    nodes: &[Node],
    edges: &[Edge],
) -> Result<Vec<weave_graph_core::policy::SemanticCouplingFinding>, Box<dyn std::error::Error>> {
    const OVERSAMPLE: usize = 8;

    let rules = load_semantic_coupling_rules(&root.join(POLICY_FILE))?;
    let scope_ids: Vec<u32> = nodes.iter().map(|n| n.id).collect();
    let mut findings = Vec::new();
    for rule in &rules {
        let pairs = storage.find_similar_node_pairs(&scope_ids, rule.threshold, OVERSAMPLE)?;
        findings.extend(weave_graph_core::policy::semantic_coupling_findings(
            nodes, edges, &pairs, rule,
        ));
    }
    Ok(findings)
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

/// The one `Storage` round-trip lint/drift both pay — callers that also
/// need the raw, unfiltered graph (POL-03's drift comparison) get it from
/// this same fetch instead of querying storage a second time.
fn fetch_graph(
    storage: &dyn weave_graph_core::Storage,
) -> Result<(Vec<Node>, Vec<Edge>), Box<dyn std::error::Error>> {
    Ok((storage.all_nodes()?, storage.all_edges()?))
}

/// Pure in-memory filter (no I/O): nodes kept per the caller's visibility
/// predicate, edges kept only when both endpoints survived. Cross-module
/// classification needs both sides; a half-visible edge is unclassifiable
/// and counted, never guessed about.
fn filter_view(
    all_nodes: Vec<Node>,
    all_edges: Vec<Edge>,
    visible: Option<&dyn Fn(&Node) -> bool>,
) -> GraphView {
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
    let mut edges = Vec::new();
    let mut skipped_edges = 0usize;
    for edge in all_edges {
        if visible_ids.contains(&edge.source_id) && visible_ids.contains(&edge.target_id) {
            edges.push(edge);
        } else {
            skipped_edges += 1;
        }
    }
    GraphView {
        nodes,
        edges,
        hidden_nodes,
        skipped_edges,
    }
}

/// Confirmed ADR obligations are
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
