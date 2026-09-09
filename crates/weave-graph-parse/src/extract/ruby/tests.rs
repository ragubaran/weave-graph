use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Ruby)
        .unwrap()
        .parse("a.rb", source)
        .unwrap()
}

#[test]
fn extracts_class_hierarchy_methods_and_calls() {
    let file = parse(
        "class Greeter\n    def initialize(name)\n        @name = name\n    end\n\n    def greet\n        format_name(@name)\n    end\nend\n\nclass LoudGreeter < Greeter\nend\n\ndef format_name(name)\n    name.strip\nend\n",
    );
    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("Greeter", SymbolKind::Class),
            ("Greeter::initialize", SymbolKind::Method),
            ("Greeter::greet", SymbolKind::Method),
            ("LoudGreeter", SymbolKind::Class),
            ("format_name", SymbolKind::Function),
        ]
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Inherits && e.target_name == "Greeter")
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "format_name" && !c.is_member_call)
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "strip" && c.is_member_call)
    );
}
