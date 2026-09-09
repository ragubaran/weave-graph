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
