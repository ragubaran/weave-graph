//! `weave_policy_lint` (CORE-03, feature `policy-lint`): same YAML shape
//! and evaluation as `weave policy lint`, wired for MCP callers instead of
//! the CLI. YAML parsing is duplicated from `weave-graph-cli::policy`
//! rather than shared — this crate depends on nothing in `weave-graph-cli`
//! (the dependency direction only ever runs the other way).
//!
//! RBAC masking follows `weave policy lint`'s own POL-01 caveat: a
//! restricted identity's clean run only means "no violations it could
//! see," never a repo-wide compliance guarantee.

use std::path::Path;

use serde::Deserialize;
use weave_graph_core::policy::{Boundary, BoundaryRule, Violation, lint};
use weave_graph_core::{Edge, Node};

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

fn validate_boundary(boundary: &Boundary) -> Result<(), String> {
    if boundary.from.is_empty() || boundary.to.is_empty() {
        return Err("boundary endpoints must be non-empty path prefixes".to_string());
    }
    Ok(())
}

/// Parses a policy YAML file into core rule values — same shape and
/// validation as `weave-graph-cli::policy::load_rules`.
pub fn load_rules(path: &Path) -> Result<Vec<BoundaryRule>, String> {
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

fn render(violations: &[Violation]) -> String {
    if violations.is_empty() {
        return "✓ no boundary violations".to_string();
    }
    let mut out = format!("✗ {} policy violation(s)\n", violations.len());
    for v in violations {
        out.push_str(&format!("[{}] {} -> {}\n", v.kind, v.from, v.to));
        for example in &v.examples {
            out.push_str(&format!("    {example}\n"));
        }
    }
    out
}

/// Loads `policy_path` and lints `nodes`/`edges` — the caller is
/// responsible for any RBAC filtering (same one-guard rule
/// `weave_graph_core::policy::lint`'s own doc comment states).
pub fn weave_policy_lint(
    policy_path: &Path,
    nodes: &[Node],
    edges: &[Edge],
) -> Result<String, String> {
    let rules = load_rules(policy_path)?;
    let violations = lint(nodes, edges, &rules);
    Ok(render(&violations))
}

#[cfg(test)]
mod tests;
