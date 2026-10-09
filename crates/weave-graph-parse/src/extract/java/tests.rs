use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Java)
        .unwrap()
        .parse("a.java", source)
        .unwrap()
}

#[test]
fn extracts_interface_class_hierarchy_and_calls() {
    let file = parse(
        "import java.util.List;\ninterface Named { String getName(); }\nclass Greeter implements Named { private String name; public String getName() { return this.name; } public String greet() { return formatName(this.getName()); } }\nclass LoudGreeter extends Greeter { }\n",
    );
    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("Named", SymbolKind::Interface),
            ("Named::getName", SymbolKind::Method),
            ("Greeter", SymbolKind::Class),
            ("Greeter::getName", SymbolKind::Method),
            ("Greeter::greet", SymbolKind::Method),
            ("LoudGreeter", SymbolKind::Class),
        ]
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Implements && e.target_name == "Named")
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Inherits && e.target_name == "Greeter")
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Imports && e.target_name == "List")
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "formatName" && !c.is_member_call)
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "getName" && c.is_member_call)
    );
}

#[cfg(feature = "framework-routes")]
mod spring_routes {
    use super::parse;
    use crate::model::SymbolKind;

    #[test]
    fn get_mapping_in_a_rest_controller_produces_a_route_symbol_and_a_routes_to_edge() {
        let file = parse(
            "@RestController\npublic class UserController {\n    @GetMapping(\"/users/{id}\")\n    public User getUser(Long id) {\n        return service.find(id);\n    }\n}\n",
        );
        let route = file
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Route)
            .expect("a route symbol must be indexed");
        assert_eq!(route.symbol, "route:GET /users/{id}");

        let edge = file
            .structural_edges
            .iter()
            .find(|e| e.kind.as_str() == "ROUTES_TO")
            .expect("a ROUTES_TO edge must be produced");
        assert_eq!(edge.source_moniker, route.moniker);
        assert_eq!(edge.target_name, "getUser");
    }

    #[test]
    fn request_mapping_with_a_method_element_is_tagged_with_its_own_method() {
        let file = parse(
            "@Controller\npublic class UserController {\n    @RequestMapping(value = \"/users\", method = RequestMethod.POST)\n    public User createUser() { return null; }\n}\n",
        );
        let route = file
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Route)
            .expect("a route symbol must be indexed");
        assert_eq!(route.symbol, "route:POST /users");
    }

    #[test]
    fn bare_request_mapping_with_no_method_element_is_any() {
        let file = parse(
            "@RestController\npublic class UserController {\n    @RequestMapping(\"/users\")\n    public User listUsers() { return null; }\n}\n",
        );
        let route = file
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Route)
            .expect("a route symbol must be indexed");
        assert_eq!(route.symbol, "route:ANY /users");
    }

    #[test]
    fn mapping_annotation_outside_a_controller_class_is_not_a_route() {
        // Same false-positive class the Flask pilot had: a mapping-shaped
        // annotation alone must never be enough — the enclosing class
        // must actually be `@RestController`/`@Controller`.
        let file = parse(
            "public class UserRepository {\n    @GetMapping(\"/users\")\n    public User getUser() { return null; }\n}\n",
        );
        assert!(
            !file.symbols.iter().any(|s| s.kind == SymbolKind::Route),
            "{file:?}"
        );
    }

    #[test]
    fn the_wrapped_handler_is_still_indexed_as_an_ordinary_method() {
        let file = parse(
            "@RestController\npublic class UserController {\n    @GetMapping(\"/ping\")\n    public String ping() { return \"ok\"; }\n}\n",
        );
        assert!(
            file.symbols
                .iter()
                .any(|s| s.symbol == "UserController::ping" && s.kind == SymbolKind::Method),
            "the route pilot must never suppress normal method indexing: {file:?}"
        );
    }

    #[test]
    fn a_plain_method_in_a_controller_without_a_mapping_annotation_is_not_a_route() {
        let file = parse(
            "@RestController\npublic class UserController {\n    public String helper() { return \"x\"; }\n}\n",
        );
        assert!(
            !file.symbols.iter().any(|s| s.kind == SymbolKind::Route),
            "{file:?}"
        );
    }
}
