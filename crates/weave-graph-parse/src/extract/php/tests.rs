use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Php)
        .unwrap()
        .parse("a.php", source)
        .unwrap()
}

#[test]
fn extracts_interface_class_hierarchy_and_calls() {
    let file = parse(
        "<?php\n\nrequire 'helpers.php';\n\ninterface Named {\n    public function getName(): string;\n}\n\nclass Greeter implements Named {\n    public function getName(): string {\n        return $this->name;\n    }\n\n    public function greet(): string {\n        return formatName($this->getName());\n    }\n}\n\nclass LoudGreeter extends Greeter {\n}\n\nfunction formatName($name) {\n    return trim($name);\n}\n",
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
            ("formatName", SymbolKind::Function),
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
            .any(|e| e.kind == StructuralEdgeKind::Imports && e.target_name == "helpers.php")
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
