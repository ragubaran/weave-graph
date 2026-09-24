//! Query-layer RBAC: one masking guard
//! shared by every consumer — CLI `query`/`report`/`export` and the MCP
//! server — so none of them can drift from the others. Building masking
//! into the export path alone, even "temporarily", is the specific
//! mistake a prior design review already caught; every consumer
//! routes through [`RbacGuard`] instead.
//!
//! [`AuthProvider`] is the only identity-resolution seam. No vendor
//! (Okta/Azure AD/SAML/OIDC) is named here — those are pluggable
//! implementations a deployment supplies; real SSO wiring is future work,
//! not implemented yet.

use std::collections::HashMap;

use crate::model::Node;
#[cfg(test)]
use crate::model::NodeId;

/// A resolved caller identity: a subject name plus zero or more roles.
/// `roles` is free-form (like `Edge::kind`) — this crate defines no fixed
/// role taxonomy. Two role names are special-cased: `"internal"`
/// ([`Identity::is_internal`]) and `"allow-drift"`
/// ([`Identity::can_waive`]) — deliberately separate privileges, since
/// seeing everything (`internal`) doesn't imply permission to bypass a CI
/// gate (`allow-drift`). Every other role name is meaningful only to the
/// caller-supplied visibility predicate (see [`RbacGuard::new`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub subject: String,
    pub roles: Vec<String>,
    /// RBAC-01: path prefixes this identity's `internal` bypass is scoped
    /// to, e.g. `["crates/weave-graph-hub/**"]`. Empty means unscoped —
    /// today's behavior, an `internal` identity sees everything
    /// `is_public` doesn't already allow. Matched by
    /// [`RbacGuard::visible`] via the same boundary-safe prefix rule
    /// `weave_graph_core::policy::in_module` uses (duplicated here, not
    /// imported: that module is gated behind the separate `policy-lint`
    /// feature, and `rbac` must not pull it in just for one helper).
    pub path_scope: Vec<String>,
}

impl Identity {
    /// The default identity when nothing authenticates: no roles, sees
    /// only what `is_public` allows.
    pub fn anonymous() -> Self {
        Self {
            subject: "anonymous".to_string(),
            roles: Vec::new(),
            path_scope: Vec::new(),
        }
    }

    /// The one role this crate treats specially: bypasses masking
    /// entirely. Any other role name is meaningful only to the
    /// caller-supplied visibility predicate (see [`RbacGuard::new`]).
    pub fn is_internal(&self) -> bool {
        self.roles.iter().any(|r| r == "internal")
    }

    /// Permission to invoke a `weave check-contracts`/
    /// `weave blast` waiver (`--allow-drift`, `--allow-drift-for`,
    /// `--skip`, or their `WEAVE_*` env-var equivalents). Independent of
    /// `is_internal` — an identity that can see every symbol isn't
    /// automatically trusted to wave through a CI gate.
    pub fn can_waive(&self) -> bool {
        self.roles.iter().any(|r| r == "allow-drift")
    }
}

/// Pluggable identity resolution: `AuthProvider`
/// implementations (Okta / Azure AD / SAML / OIDC) supply identity; no
/// named vendor appears in the core. `credential` is opaque to this
/// trait — a bearer token, a config-file subject name, whatever the
/// concrete implementation expects.
pub trait AuthProvider {
    fn resolve(&self, credential: Option<&str>) -> Identity;
}

/// The simplest possible [`AuthProvider`]: a static subject -> roles map.
/// Good enough for local config-driven deployments and for this
/// milestone's own tests — real SSO connectors are a separate, pluggable
/// `AuthProvider` implementation this crate deliberately doesn't own.
#[derive(Debug, Default, Clone)]
pub struct StaticAuthProvider {
    /// `(roles, path_scope)` per subject — `path_scope` empty unless
    /// [`StaticAuthProvider::with_path_scopes`] set it (RBAC-01).
    users: HashMap<String, (Vec<String>, Vec<String>)>,
}

impl StaticAuthProvider {
    pub fn new(users: HashMap<String, Vec<String>>) -> Self {
        Self {
            users: users
                .into_iter()
                .map(|(subject, roles)| (subject, (roles, Vec::new())))
                .collect(),
        }
    }

    /// RBAC-01: same shape as [`StaticAuthProvider::new`], plus each
    /// subject's `path_scope`. A separate constructor rather than
    /// widening `new`'s own signature, so every existing caller (and
    /// test) stays byte-for-byte unaffected.
    pub fn with_path_scopes(users: HashMap<String, (Vec<String>, Vec<String>)>) -> Self {
        Self { users }
    }
}

impl AuthProvider for StaticAuthProvider {
    fn resolve(&self, credential: Option<&str>) -> Identity {
        match credential.and_then(|subject| self.users.get(subject).map(|entry| (subject, entry))) {
            Some((subject, (roles, path_scope))) => Identity {
                subject: subject.to_string(),
                roles: roles.clone(),
                path_scope: path_scope.clone(),
            },
            None => Identity::anonymous(),
        }
    }
}

/// Boundary-safe path-prefix match — `path` is `prefix` itself, or lives
/// under `prefix/`, never a sibling whose name merely starts with it
/// (`crates/weave-graph-hub` must not swallow `crates/weave-graph-hub2`).
/// Byte-for-byte the same rule as `weave_graph_core::policy::in_module`;
/// duplicated rather than imported (see [`Identity::path_scope`]'s own
/// doc comment for why).
fn in_module(path: &str, prefix: &str) -> bool {
    path == prefix || prefix.is_empty() || path.starts_with(&format!("{prefix}/"))
}

/// Opaque contract-boundary stand-in: a hidden node's
/// `id` survives (edges still resolve to *something*), everything that
/// would leak internals — signature, exact path, line numbers — is
/// replaced by a fixed marker rather than merely annotated.
const HIDDEN_MARKER: &str = "<rbac: hidden>";

/// One masking guard, constructed once per request/session and shared by
/// every consumer. `is_public` is supplied by the
/// caller rather than hard-coded here: "public API surface" is a
/// language-aware heuristic (`weave-graph-parse::contract`) this crate
/// has no dependency on — `weave-graph-parse` depends on
/// `weave-graph-core`, never the reverse.
///
/// Deliberately does *not* filter `Vec<Node>` positionally: several
/// consumers resolve a `CsrGraph`'s compact indices back into a node
/// list built from the same `Storage::all_nodes()` call, and dropping
/// entries would desync that correspondence. [`RbacGuard::mask_node`]
/// masks content in place instead, preserving list length and order.
pub struct RbacGuard {
    identity: Identity,
    is_public: Box<dyn Fn(&Node) -> bool>,
}

impl RbacGuard {
    pub fn new(identity: Identity, is_public: impl Fn(&Node) -> bool + 'static) -> Self {
        Self {
            identity,
            is_public: Box::new(is_public),
        }
    }

    /// Everyone with the `internal` role sees everything, unless
    /// `path_scope` narrows that (RBAC-01): a scoped identity's bypass
    /// only covers nodes under one of its listed prefixes, falling back
    /// to `is_public` outside that scope exactly like a non-internal
    /// identity would. An unscoped `internal` identity (`path_scope`
    /// empty) is unaffected — today's unconditional bypass.
    pub fn visible(&self, node: &Node) -> bool {
        if self.identity.is_internal() {
            if self.identity.path_scope.is_empty() {
                return true;
            }
            if self
                .identity
                .path_scope
                .iter()
                .any(|prefix| in_module(&node.path, prefix))
            {
                return true;
            }
        }
        (self.is_public)(node)
    }

    /// Does this identity carry the `allow-drift` role
    /// (see [`Identity::can_waive`])? A guard-level passthrough so
    /// callers checking waiver permission don't need to reach into
    /// `Identity` directly.
    pub fn can_waive(&self) -> bool {
        self.identity.can_waive()
    }

    /// This identity's raw role list — POL-04's exemption check
    /// (`weave_graph_core::policy::lint_scoped`) reads it directly rather
    /// than this crate re-deriving its own "does this role apply" logic.
    pub fn roles(&self) -> &[String] {
        &self.identity.roles
    }

    /// Masks a single node for output: unchanged if visible, an opaque
    /// contract-boundary stand-in otherwise (see [`HIDDEN_MARKER`]).
    pub fn mask_node(&self, node: &Node) -> Node {
        if self.visible(node) {
            return node.clone();
        }
        Node {
            id: node.id,
            repo_id: node.repo_id.clone(),
            path: HIDDEN_MARKER.to_string(),
            symbol: HIDDEN_MARKER.to_string(),
            kind: HIDDEN_MARKER.to_string(),
            line_start: 0,
            line_end: 0,
            signature: String::new(),
        }
    }
}

#[cfg(test)]
mod tests;
