//! CLI glue for M3.0's query-layer RBAC: resolves an identity from
//! `.weave/config.toml`'s `[rbac.users]` table and builds the one
//! `RbacGuard` every masked command (`query`/`report`/`export`, and
//! `serve --mcp`) shares — see `weave_graph_core::rbac` for the guard
//! itself and why masking lives there, not here.

use std::path::Path;

use weave_graph_core::Node;
use weave_graph_core::rbac::{AuthProvider, Identity, RbacGuard, StaticAuthProvider};
use weave_graph_parse::Language;
use weave_graph_parse::contract::{short_name, visibility_rule};

use crate::config::read_rbac_users;

/// "Is this node part of the public API surface" — reuses the same
/// per-language heuristic `contract.rs` uses for the M2.2 contract hash,
/// so RBAC visibility and that gate never disagree. A path with no
/// recognized language is treated as fully internal — the safer default
/// for an extension this crate can't classify.
fn is_public(node: &Node) -> bool {
    match Language::from_path(Path::new(&node.path)) {
        Some(language) => {
            let rule = visibility_rule(language);
            rule(node.signature.trim(), short_name(&node.symbol))
        }
        None => false,
    }
}

/// Resolves `--as <subject>` against `.weave/config.toml`'s `[rbac.users]`
/// map and builds the guard every masked consumer shares. `as_subject =
/// None` (no `--as` flag) resolves to the anonymous identity — no roles,
/// the safe default when a command doesn't opt in to an identity.
pub(crate) fn guard_for(root: &Path, as_subject: Option<&str>) -> RbacGuard {
    let config_path = root.join(".weave").join("config.toml");
    let users = read_rbac_users(&config_path);
    let identity: Identity = StaticAuthProvider::new(users).resolve(as_subject);
    RbacGuard::new(identity, is_public)
}

#[cfg(test)]
mod tests;
