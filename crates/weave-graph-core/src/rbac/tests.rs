use super::*;

fn node(id: NodeId, symbol: &str) -> Node {
    Node {
        id,
        repo_id: "local".to_string(),
        path: "src/lib.rs".to_string(),
        symbol: symbol.to_string(),
        kind: "function".to_string(),
        line_start: 1,
        line_end: 2,
        signature: format!("fn {symbol}()"),
    }
}

fn is_pub_prefixed(node: &Node) -> bool {
    node.symbol.starts_with("pub_")
}

#[test]
fn internal_identity_sees_everything() {
    let identity = Identity {
        subject: "alice".to_string(),
        roles: vec!["internal".to_string()],
    };
    let guard = RbacGuard::new(identity, is_pub_prefixed);
    let hidden = node(1, "secret_helper");
    assert!(guard.visible(&hidden));
    assert_eq!(guard.mask_node(&hidden), hidden);
}

#[test]
fn external_identity_only_sees_public_nodes() {
    let identity = Identity::anonymous();
    let guard = RbacGuard::new(identity, is_pub_prefixed);

    let public = node(1, "pub_entry");
    let private = node(2, "secret_helper");

    assert!(guard.visible(&public));
    assert!(!guard.visible(&private));
    assert_eq!(guard.mask_node(&public), public);
}

#[test]
fn masked_node_hides_content_but_keeps_id() {
    let identity = Identity::anonymous();
    let guard = RbacGuard::new(identity, is_pub_prefixed);
    let private = node(7, "secret_helper");

    let masked = guard.mask_node(&private);
    assert_eq!(masked.id, 7);
    assert_ne!(masked.symbol, private.symbol);
    assert_ne!(masked.path, private.path);
    assert!(masked.signature.is_empty());
    assert_eq!(masked.line_start, 0);
    assert_eq!(masked.line_end, 0);
}

#[test]
fn can_waive_is_independent_of_is_internal() {
    // `internal` (visibility) and `allow-drift` (waiver permission) are
    // deliberately separate privileges — neither implies the other.
    let internal_only = Identity {
        subject: "alice".to_string(),
        roles: vec!["internal".to_string()],
    };
    assert!(internal_only.is_internal());
    assert!(!internal_only.can_waive());

    let waiver_only = Identity {
        subject: "release-bot".to_string(),
        roles: vec!["allow-drift".to_string()],
    };
    assert!(!waiver_only.is_internal());
    assert!(waiver_only.can_waive());

    assert!(!Identity::anonymous().can_waive());
}

#[test]
fn rbac_guard_can_waive_passes_through_the_bound_identity() {
    let identity = Identity {
        subject: "release-bot".to_string(),
        roles: vec!["allow-drift".to_string()],
    };
    let guard = RbacGuard::new(identity, is_pub_prefixed);
    assert!(guard.can_waive());

    let guard = RbacGuard::new(Identity::anonymous(), is_pub_prefixed);
    assert!(!guard.can_waive());
}

#[test]
fn static_auth_provider_resolves_known_and_unknown_subjects() {
    let mut users = HashMap::new();
    users.insert("alice".to_string(), vec!["internal".to_string()]);
    let provider = StaticAuthProvider::new(users);

    let alice = provider.resolve(Some("alice"));
    assert_eq!(alice.subject, "alice");
    assert!(alice.is_internal());

    let bob = provider.resolve(Some("bob"));
    assert_eq!(bob, Identity::anonymous());

    let none = provider.resolve(None);
    assert_eq!(none, Identity::anonymous());
}
