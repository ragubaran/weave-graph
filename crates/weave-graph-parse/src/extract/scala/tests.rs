use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Scala)
        .unwrap()
        .parse("a.scala", source)
        .unwrap()
}

#[test]
fn extracts_trait_class_hierarchy_and_calls() {
    let file = parse(
        "import scala.collection.List\n\ntrait Named {\n    def getName(): String\n}\n\nclass Greeter(name: String) extends Named {\n    def getName(): String = {\n        this.name\n    }\n\n    def greet(): String = {\n        formatName(this.getName())\n    }\n}\n",
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
