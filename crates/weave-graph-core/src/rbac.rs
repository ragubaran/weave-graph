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
}

impl Identity {
    /// The default identity when nothing authenticates: no roles, sees
    /// only what `is_public` allows.
    pub fn anonymous() -> Self {
        Self {
            subject: "anonymous".to_string(),
            roles: Vec::new(),
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
    users: HashMap<String, Vec<String>>,
}

impl StaticAuthProvider {
    pub fn new(users: HashMap<String, Vec<String>>) -> Self {
        Self { users }
    }
}

impl AuthProvider for StaticAuthProvider {
    fn resolve(&self, credential: Option<&str>) -> Identity {
        match credential.and_then(|subject| self.users.get(subject).map(|roles| (subject, roles))) {
            Some((subject, roles)) => Identity {
                subject: subject.to_string(),
                roles: roles.clone(),
            },
            None => Identity::anonymous(),
        }
    }
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

    /// Everyone with the `internal` role sees everything; everyone else
    /// sees only what `is_public` allows.
    pub fn visible(&self, node: &Node) -> bool {
        self.identity.is_internal() || (self.is_public)(node)
    }

    /// Does this identity carry the `allow-drift` role
    /// (see [`Identity::can_waive`])? A guard-level passthrough so
    /// callers checking waiver permission don't need to reach into
    /// `Identity` directly.
    pub fn can_waive(&self) -> bool {
        self.identity.can_waive()
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
