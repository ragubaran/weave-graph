use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Go)
        .unwrap()
        .parse("a.go", source)
        .unwrap()
}

#[test]
fn extracts_struct_interface_method_and_function() {
    let file = parse(
        "package main\nimport \"fmt\"\ntype Greeter struct { Name string }\nfunc (g *Greeter) Greet() string { return formatName(g.Name) }\nfunc formatName(name string) string { fmt.Println(name); return name }\ntype Named interface { GetName() string }\n",
    );
    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("Greeter", SymbolKind::Struct),
            ("Greeter::Greet", SymbolKind::Method),
            ("formatName", SymbolKind::Function),
            ("Named", SymbolKind::Interface),
        ]
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "formatName" && !c.is_member_call)
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "Println" && c.is_member_call)
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Imports && e.target_name == "fmt")
    );
}

#[cfg(feature = "framework-routes")]
mod gin_routes {
    use super::parse;
    use crate::model::SymbolKind;

    #[test]
    fn gin_get_produces_a_route_symbol_and_a_routes_to_edge() {
        let file =
            parse("func main() {\n\tr := gin.Default()\n\tr.GET(\"/users/:id\", getUser)\n}\n");
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
        assert_eq!(edge.target_name, "getUser");
    }

    #[test]
    fn gin_new_is_also_a_recognized_constructor() {
        let file = parse("func main() {\n\tr := gin.New()\n\tr.POST(\"/users\", createUser)\n}\n");
        let route = file
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Route)
            .expect("a route symbol must be indexed");
        assert_eq!(route.symbol, "route:POST /users");
    }

    #[test]
    fn multiple_routes_on_the_same_engine_each_produce_their_own_symbol() {
        let file = parse(
            "func main() {\n\tr := gin.Default()\n\tr.GET(\"/a\", a)\n\tr.DELETE(\"/b\", b)\n}\n",
        );
        let mut routes: Vec<&str> = file
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Route)
            .map(|s| s.symbol.as_str())
            .collect();
        routes.sort_unstable();
        assert_eq!(routes, vec!["route:DELETE /b", "route:GET /a"], "{file:?}");
    }

    #[test]
    fn a_non_gin_receiver_with_the_same_call_shape_is_not_a_route() {
        // Same false-positive class the Flask pilot had: a `.GET(...)`
        // call alone must never be enough — its receiver must actually
        // have been assigned from `gin.Default()`/`gin.New()`.
        let file = parse(
            "func main() {\n\tclient := http.DefaultClient\n\tclient.GET(\"/x\", handler)\n}\n",
        );
        assert!(
            !file.symbols.iter().any(|s| s.kind == SymbolKind::Route),
            "{file:?}"
        );
    }

    #[test]
    fn the_wrapped_constructor_call_is_still_collected_as_an_ordinary_call() {
        let file = parse("func main() {\n\tr := gin.Default()\n\tr.GET(\"/x\", handler)\n}\n");
        assert!(
            file.calls.iter().any(|c| c.callee_name == "Default"),
            "the route pilot must never suppress ordinary call collection: {file:?}"
        );
    }

    #[test]
    fn a_non_identifier_handler_argument_is_not_a_guess() {
        let file = parse(
            "func main() {\n\tr := gin.Default()\n\tr.GET(\"/x\", func(c *gin.Context) {})\n}\n",
        );
        assert!(
            !file.symbols.iter().any(|s| s.kind == SymbolKind::Route),
            "{file:?}"
        );
    }
}
