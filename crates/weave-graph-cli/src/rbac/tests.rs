use std::fs;

use weave_graph_core::Node;

use super::*;

fn node(path: &str, symbol: &str, signature: &str) -> Node {
    Node {
        id: 1,
        repo_id: "local".to_string(),
        path: path.to_string(),
        symbol: symbol.to_string(),
        kind: "function".to_string(),
        line_start: 1,
        line_end: 2,
        signature: signature.to_string(),
    }
}

#[test]
fn is_public_uses_the_per_language_contract_heuristic() {
    let public = node("src/lib.rs", "run", "pub fn run()");
    let private = node("src/lib.rs", "helper", "fn helper()");
    assert!(is_public(&public));
    assert!(!is_public(&private));
}

#[test]
fn is_public_treats_an_unrecognized_extension_as_internal() {
    let unknown = node("data/notes.xyz", "whatever", "pub fn whatever()");
    assert!(!is_public(&unknown));
}

#[test]
fn guard_for_unconfigured_subject_resolves_to_anonymous() {
    let dir = tempfile::tempdir().unwrap();
    let guard = guard_for(dir.path(), Some("nobody"));
    let private = node("src/lib.rs", "helper", "fn helper()");
    assert!(!guard.visible(&private));
}

#[test]
fn guard_for_configured_internal_subject_sees_everything() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".weave")).unwrap();
    fs::write(
        dir.path().join(".weave/config.toml"),
        "[rbac.users]\nalice = [\"internal\"]\n",
    )
    .unwrap();
    let guard = guard_for(dir.path(), Some("alice"));
    let private = node("src/lib.rs", "helper", "fn helper()");
    assert!(guard.visible(&private));
}
