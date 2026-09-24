//! HUB-03: mesh-wide policy-lint endpoint (feature `hub-policy-lint`,
//! sequenced strictly after FED-01 per `docs/proposal-skylos.md` §6.6).
//! Reuses `weave_graph_core::policy::lint` and the same
//! scratch-file-then-open-read-only pattern `canvas::from_snapshot_bytes`
//! already uses for one snapshot's raw bytes. YAML parsing is duplicated
//! from `weave-graph-cli::policy`/`weave-graph-mcp::policy_lint` rather
//! than shared — this crate depends on nothing in either (wrong
//! dependency direction), the same convention those two already follow
//! with each other.
//!
//! There is no per-repo `.weave/policy.yaml` concept in the hub (repos
//! only ever push a `graph.db` snapshot, never their working tree). The
//! mesh policy instead lives once per registry, at `<store_dir>/policy.yaml`
//! — an operator-configured, mesh-wide boundary set, analogous to
//! `RegistryConfig::canvas_exclude`.

use std::path::Path;

use serde::Deserialize;
use weave_graph_core::Storage;
use weave_graph_core::policy::{Boundary, BoundaryRule, Violation};
use weave_graph_store_sqlite::SqliteStorage;

const REGISTRY_POLICY_FILE: &str = "policy.yaml";

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
    #[serde(default)]
    allowed_roles: Vec<String>,
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

/// Loads `<store_dir>/policy.yaml`. A missing file is zero rules, not an
/// error — a registry that never configured mesh policy sees no behavior
/// change, same "absent means nothing declared" convention
/// `weave-graph-cli::policy::load_rules` already uses for its own file.
pub(crate) fn load_rules(store_dir: &Path) -> Result<Vec<BoundaryRule>, String> {
    let path = store_dir.join(REGISTRY_POLICY_FILE);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    let parsed: PolicyFile =
        serde_yaml::from_str(&content).map_err(|e| format!("invalid policy YAML: {e}"))?;
    let mut rules = Vec::new();
    for entry in &parsed.rules {
        let disallow = entry.disallow.as_ref().map(to_boundary);
        let require = entry.require.as_ref().map(to_boundary);
        match (disallow, require) {
            (Some(_), Some(_)) => {
                return Err("a rule cannot be both `disallow` and `require`".to_string());
            }
            (Some(b), None) => rules.push(BoundaryRule::Disallow(b)),
            (None, Some(b)) => rules.push(BoundaryRule::Require(b)),
            (None, None) => {
                return Err("each rule needs exactly one of `disallow` or `require`".to_string());
            }
        }
    }
    Ok(rules)
}

fn scratch_nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Lints one snapshot's raw bytes against `rules` — the same
/// write-to-scratch-then-open-read-only pattern
/// `canvas::from_snapshot_bytes` uses, duplicated rather than shared
/// since that one returns a `Canvas`, this one raw violations, and
/// neither needs the other's shape.
pub(crate) fn lint_snapshot_bytes(
    bytes: &[u8],
    rules: &[BoundaryRule],
) -> Result<Vec<Violation>, String> {
    let scratch = std::env::temp_dir().join(format!(
        "weave-hub-policy-lint-{}-{}.db",
        std::process::id(),
        scratch_nonce()
    ));
    let result = (|| {
        std::fs::write(&scratch, bytes).map_err(|e| e.to_string())?;
        let storage = SqliteStorage::open_read_only(&scratch).map_err(|e| e.to_string())?;
        let nodes = storage.all_nodes().map_err(|e| e.to_string())?;
        let edges = storage.all_edges().map_err(|e| e.to_string())?;
        Ok(weave_graph_core::policy::lint(&nodes, &edges, rules))
    })();
    let _ = std::fs::remove_file(&scratch);
    result
}

#[cfg(test)]
mod tests;
