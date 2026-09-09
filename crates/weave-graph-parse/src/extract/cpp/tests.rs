use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Cpp)
        .unwrap()
        .parse("a.cpp", source)
        .unwrap()
}

#[test]
fn extracts_class_hierarchy_methods_and_calls() {
    let file = parse(
        "class Base {\npublic:\n    virtual void name();\n};\nclass Greeter : public Base {\npublic:\n    std::string greet() {\n        return this->helper();\n    }\n    std::string helper() {\n        return formatName();\n    }\n};\n",
    );
    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("Base", SymbolKind::Class),
            ("Greeter", SymbolKind::Class),
            ("Greeter::greet", SymbolKind::Method),
            ("Greeter::helper", SymbolKind::Method)
        ]
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Inherits && e.target_name == "Base")
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "helper" && c.is_member_call)
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "formatName" && !c.is_member_call)
    );
}
