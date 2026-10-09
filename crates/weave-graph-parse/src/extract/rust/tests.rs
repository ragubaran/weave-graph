use crate::language::Language;
use crate::model::StructuralEdgeKind;
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Rust)
        .unwrap()
        .parse("a.rs", source)
        .unwrap()
}

#[test]
fn nested_modules_qualify_symbols_by_full_module_path() {
    let file = parse("mod outer { fn f() {} mod inner { fn g() {} } }");
    let names: Vec<&str> = file.symbols.iter().map(|s| s.symbol.as_str()).collect();
    assert_eq!(names, vec!["outer::f", "outer::inner::g"]);
}

#[test]
fn grouped_use_list_produces_one_import_per_leaf() {
    let file = parse("use std::{fmt, collections::HashMap};");
    let mut targets: Vec<&str> = file
        .structural_edges
        .iter()
        .map(|e| e.target_name.as_str())
        .collect();
    targets.sort_unstable();
    assert_eq!(targets, vec!["HashMap", "fmt"]);
    assert!(
        file.structural_edges
            .iter()
            .all(|e| e.kind == StructuralEdgeKind::Imports)
    );
}

#[test]
fn use_as_clause_imports_the_original_name_not_the_alias() {
    let file = parse("use std::collections::HashMap as Map;");
    assert_eq!(file.structural_edges[0].target_name, "HashMap");
}

#[test]
fn fully_qualified_call_resolves_to_its_final_segment() {
    let file = parse("fn caller() { std::cmp::max(1, 2); }");
    assert_eq!(file.calls.len(), 1);
    assert_eq!(file.calls[0].callee_name, "max");
    assert!(!file.calls[0].is_member_call);
}

#[test]
fn calling_through_a_parenthesized_closure_is_not_treated_as_a_named_call() {
    let file = parse("fn caller() { (|x: i32| x)(5); }");
    assert!(
        file.calls.is_empty(),
        "no static callee name exists to record"
    );
}

// `collect_calls` only records an edge from a `call_expression`'s own
// callee. A function passed by value — bound, mapped, pushed into a Vec —
// never appears as a callee, so no edge of any kind records it; "who uses
// this function" queries silently miss it.
#[test]
fn referencing_a_function_as_a_value_without_calling_it_produces_no_edge() {
    let file = parse(
        "fn my_fn(x: i32) -> i32 { x }\n\
         fn caller() {\n\
         let _bound = my_fn;\n\
         let v: Vec<i32> = Vec::new();\n\
         let _ = v.into_iter().map(my_fn);\n\
         let mut callbacks: Vec<fn(i32) -> i32> = Vec::new();\n\
         callbacks.push(my_fn);\n\
         }\n",
    );
    assert_eq!(file.symbols.len(), 2, "both functions are still indexed");
    assert!(
        file.calls.iter().all(|c| c.callee_name != "my_fn"),
        "my_fn is never invoked, so no CALLS_* edge should name it"
    );
    assert!(
        file.structural_edges
            .iter()
            .all(|e| e.target_name != "my_fn"),
        "no structural edge kind (IMPORTS/INHERITS/IMPLEMENTS) covers a by-value reference either"
    );
}

#[cfg(feature = "framework-routes")]
mod axum_routes {
    use super::parse;
    use crate::model::SymbolKind;

    #[test]
    fn router_new_chain_produces_a_route_symbol_and_a_routes_to_edge() {
        let file = parse(
            "fn build() -> Router {\n    Router::new()\n        .route(\"/users/:id\", get(get_user))\n}\n",
        );
        let route = file
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Route)
            .expect("a route symbol must be indexed");
        assert_eq!(route.symbol, "route:GET /users/:id");

        let edge = file
            .structural_edges
            .iter()
            .find(|e| e.kind.as_str() == "ROUTES_TO")
            .expect("a ROUTES_TO edge must be produced");
        assert_eq!(edge.source_moniker, route.moniker);
        assert_eq!(edge.target_name, "get_user");
    }

    #[test]
    fn multiple_chained_routes_each_produce_their_own_symbol() {
        let file = parse(
            "fn build() -> Router {\n    Router::new()\n        .route(\"/a\", get(a))\n        .route(\"/b\", post(b))\n}\n",
        );
        let mut routes: Vec<&str> = file
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Route)
            .map(|s| s.symbol.as_str())
            .collect();
        routes.sort_unstable();
        assert_eq!(routes, vec!["route:GET /a", "route:POST /b"], "{file:?}");
    }

    #[test]
    fn qualified_axum_router_new_is_also_recognized() {
        let file = parse(
            "fn build() -> axum::Router {\n    axum::Router::new().route(\"/x\", get(h))\n}\n",
        );
        assert!(
            file.symbols.iter().any(|s| s.kind == SymbolKind::Route),
            "{file:?}"
        );
    }

    #[test]
    fn http_verb_is_tagged_from_the_wrapping_call() {
        let file = parse("fn build() -> Router {\n    Router::new().route(\"/x\", delete(h))\n}\n");
        let route = file
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Route)
            .unwrap();
        assert_eq!(route.symbol, "route:DELETE /x");
    }

    #[test]
    fn dot_route_on_an_unrelated_builder_is_not_a_route() {
        // Same false-positive class the Flask pilot had: a `.route(...)`
        // call alone must never be enough — its receiver chain must
        // actually trace back to a literal `Router::new()`.
        let file = parse(
            "fn build() {\n    let mut graph = PathBuilder::new();\n    graph.route(\"/x\", get(h));\n}\n",
        );
        assert!(
            !file.symbols.iter().any(|s| s.kind == SymbolKind::Route),
            "{file:?}"
        );
    }

    #[test]
    fn the_wrapped_handler_is_still_collected_as_an_ordinary_call() {
        let file =
            parse("fn build() -> Router {\n    Router::new().route(\"/x\", get(handler))\n}\n");
        assert!(
            file.calls.iter().any(|c| c.callee_name == "get"),
            "the route pilot must never suppress ordinary call collection: {file:?}"
        );
    }

    #[test]
    fn a_non_identifier_handler_argument_is_not_a_guess() {
        // A closure handler — this pilot never guesses at a dynamic value.
        let file = parse(
            "fn build() -> Router {\n    Router::new().route(\"/x\", get(|| async { \"ok\" }))\n}\n",
        );
        assert!(
            !file.symbols.iter().any(|s| s.kind == SymbolKind::Route),
            "{file:?}"
        );
    }
}
