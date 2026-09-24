use crate::language::Language;
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Python)
        .unwrap()
        .parse("a.py", source)
        .unwrap()
}

#[test]
fn from_import_with_multiple_names_produces_one_edge_per_name() {
    let file = parse("from os import path, sep\n");
    let mut targets: Vec<&str> = file
        .structural_edges
        .iter()
        .map(|e| e.target_name.as_str())
        .collect();
    targets.sort_unstable();
    assert_eq!(targets, vec!["path", "sep"]);
}

#[test]
fn dotted_import_uses_the_final_segment() {
    let file = parse("import os.path\n");
    assert_eq!(file.structural_edges[0].target_name, "path");
}

#[test]
fn aliased_import_uses_the_original_name_not_the_alias() {
    let file = parse("from collections import OrderedDict as OD\n");
    assert_eq!(file.structural_edges[0].target_name, "OrderedDict");
}

#[test]
fn bare_single_segment_import_resolves_via_plain_identifier() {
    let file = parse("import os\n");
    assert_eq!(file.structural_edges[0].target_name, "os");
}

#[test]
fn star_import_produces_no_structural_edge() {
    let file = parse("from os import *\n");
    assert!(file.structural_edges.is_empty());
}

#[test]
fn call_through_a_subscript_is_not_recorded_as_a_named_call() {
    let file = parse("def f():\n    handlers[0]()\n");
    assert!(file.calls.is_empty());
}

// --- P10.8: Flask route -> handler pilot (feature `framework-routes`) ---

#[cfg(feature = "framework-routes")]
mod flask_routes {
    use super::parse;
    use crate::model::SymbolKind;

    #[test]
    fn app_route_decorator_produces_a_route_symbol_and_a_routes_to_edge() {
        let file =
            parse("@app.route(\"/users/<id>\", methods=[\"GET\"])\ndef get_user(id):\n    pass\n");
        let route = file
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Route)
            .expect("a route symbol must be indexed");
        assert_eq!(route.symbol, "route:ANY /users/<id>");

        let edge = file
            .structural_edges
            .iter()
            .find(|e| e.kind.as_str() == "ROUTES_TO")
            .expect("a ROUTES_TO edge must be produced");
        assert_eq!(edge.source_moniker, route.moniker);
        assert_eq!(edge.target_name, "get_user");
    }

    #[test]
    fn http_method_shorthand_decorator_is_tagged_with_its_own_method() {
        let file = parse("@app.get(\"/health\")\ndef health():\n    pass\n");
        let route = file
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Route)
            .unwrap();
        assert_eq!(route.symbol, "route:GET /health");
        let edge = file
            .structural_edges
            .iter()
            .find(|e| e.kind.as_str() == "ROUTES_TO")
            .unwrap();
        assert_eq!(edge.target_name, "health");
    }

    #[test]
    fn the_wrapped_handler_is_still_indexed_as_an_ordinary_function() {
        let file = parse("@app.route(\"/ping\")\ndef ping():\n    pass\n");
        assert!(
            file.symbols
                .iter()
                .any(|s| s.symbol == "ping" && s.kind == SymbolKind::Function),
            "the route pilot must never suppress normal function indexing: {file:?}"
        );
    }

    #[test]
    fn calls_inside_a_decorated_handler_are_still_collected() {
        let file =
            parse("@app.route(\"/ping\")\ndef ping():\n    helper()\ndef helper():\n    pass\n");
        assert!(
            file.calls.iter().any(|c| c.callee_name == "helper"),
            "{file:?}"
        );
    }

    #[test]
    fn an_unrelated_decorator_produces_no_route_symbol() {
        let file = parse("@staticmethod\ndef helper():\n    pass\n");
        assert!(
            !file.symbols.iter().any(|s| s.kind == SymbolKind::Route),
            "{file:?}"
        );
    }

    #[test]
    fn a_decorator_call_with_no_string_literal_argument_is_not_a_route() {
        let file = parse("@app.route(build_path())\ndef handler():\n    pass\n");
        assert!(
            !file.symbols.iter().any(|s| s.kind == SymbolKind::Route),
            "{file:?}"
        );
    }

    #[test]
    fn a_decorated_class_is_not_treated_as_a_route() {
        let file = parse("@dataclass\nclass Point:\n    x: int\n");
        assert!(
            !file.symbols.iter().any(|s| s.kind == SymbolKind::Route),
            "{file:?}"
        );
        assert!(
            file.symbols
                .iter()
                .any(|s| s.symbol == "Point" && s.kind == SymbolKind::Class)
        );
    }
}
