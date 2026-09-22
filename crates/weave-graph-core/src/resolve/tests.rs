use super::{format_not_found, resolve_symbol};
use crate::Node;

fn node(id: u32, symbol: &str) -> Node {
    Node {
        id,
        repo_id: "r".into(),
        path: "a.rs".into(),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start: 1,
        line_end: 2,
        signature: format!("fn {symbol}()"),
    }
}

#[test]
fn exact_match_wins_over_every_fallback() {
    let nodes = [node(1, "AuthService::verify"), node(2, "verify")];
    assert_eq!(resolve_symbol(&nodes, "verify"), Ok(2));
}

#[test]
fn exact_match_picks_the_first_on_a_collision() {
    let nodes = [node(1, "dup"), node(2, "dup")];
    assert_eq!(resolve_symbol(&nodes, "dup"), Ok(1));
}

#[test]
fn case_insensitive_match_resolves_a_wrong_case_query() {
    let nodes = [node(1, "AuthService::Verify")];
    assert_eq!(resolve_symbol(&nodes, "authservice::verify"), Ok(1));
}

#[test]
fn unambiguous_short_name_resolves_via_qualifier_suffix() {
    let nodes = [node(1, "AuthService::verify"), node(2, "unrelated")];
    assert_eq!(resolve_symbol(&nodes, "verify"), Ok(1));
}

#[test]
fn ambiguous_short_name_surfaces_every_candidate_not_just_the_first() {
    let nodes = [
        node(1, "AuthService::verify"),
        node(2, "TokenService::verify"),
        node(3, "unrelated"),
    ];
    let Err(suggestions) = resolve_symbol(&nodes, "verify") else {
        panic!("expected an ambiguous short-name miss");
    };
    assert_eq!(suggestions.len(), 2);
    assert!(suggestions.contains(&"AuthService::verify".to_string()));
    assert!(suggestions.contains(&"TokenService::verify".to_string()));
}

#[test]
fn a_typo_resolves_via_edit_distance_suggestion() {
    let nodes = [node(1, "checkJwtExpiry"), node(2, "renderPageLayout")];
    let Err(suggestions) = resolve_symbol(&nodes, "checkJwtExpiryy") else {
        panic!("expected a near-miss");
    };
    assert_eq!(suggestions[0], "checkJwtExpiry");
}

#[test]
fn a_genuinely_nonexistent_symbol_returns_a_bounded_suggestion_list() {
    let nodes: Vec<Node> = (0..50).map(|i| node(i, &format!("symbol{i}"))).collect();
    let Err(suggestions) = resolve_symbol(&nodes, "totally_unrelated_query") else {
        panic!("expected a miss");
    };
    assert!(
        suggestions.len() <= 5,
        "must never dump the full symbol table"
    );
    assert!(!suggestions.is_empty());
}

#[test]
fn format_not_found_appends_suggestions_when_present() {
    assert_eq!(
        format_not_found("verify", &["AuthService::verify".to_string()]),
        "symbol not found: verify (did you mean: AuthService::verify?)"
    );
    assert_eq!(format_not_found("verify", &[]), "symbol not found: verify");
}
