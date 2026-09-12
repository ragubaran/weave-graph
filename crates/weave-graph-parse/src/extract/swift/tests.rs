use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Swift)
        .unwrap()
        .parse("a.swift", source)
        .unwrap()
}

#[test]
fn extracts_protocol_class_hierarchy_and_calls() {
    let file = parse(
        "import Foundation\n\nfunc standaloneHelper() -> Int {\n    return getHandler()()\n}\n\nprotocol Named {\n    func getName() -> String\n}\n\nclass Greeter: Named {\n    func getName() -> String {\n        return self.name\n    }\n\n    func greet() -> String {\n        return formatName(self.getName())\n    }\n}\n",
    );
    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("standaloneHelper", SymbolKind::Function),
            ("Named", SymbolKind::Interface),
            ("Named::getName", SymbolKind::Method),
            ("Greeter", SymbolKind::Class),
            ("Greeter::getName", SymbolKind::Method),
            ("Greeter::greet", SymbolKind::Method),
        ]
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Inherits && e.target_name == "Named")
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Imports && e.target_name == "Foundation")
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
    // `getHandler()()` calls the *result* of a call — its callee is itself a
    // call_expression, neither `navigation_expression` nor `simple_identifier`,
    // exercising `collect_calls`'s wildcard arm (silently skipped, by design:
    // this extractor only resolves direct-name and member calls). The nested
    // `getHandler()` call is still found via the unconditional recursion.
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "getHandler" && !c.is_member_call)
    );
}
