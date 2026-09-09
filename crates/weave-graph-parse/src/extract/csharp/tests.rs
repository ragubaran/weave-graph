use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::CSharp)
        .unwrap()
        .parse("a.cs", source)
        .unwrap()
}

#[test]
fn extracts_namespaced_interface_class_and_calls() {
    let file = parse(
        "using System;\nnamespace App {\n    interface INamed {\n        string GetName();\n    }\n    class Greeter : INamed {\n        public string GetName() {\n            return this.name;\n        }\n        public string Greet() {\n            return FormatName(this.GetName());\n        }\n    }\n}\n",
    );
    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("App::INamed", SymbolKind::Interface),
            ("App::INamed::GetName", SymbolKind::Method),
            ("App::Greeter", SymbolKind::Class),
            ("App::Greeter::GetName", SymbolKind::Method),
            ("App::Greeter::Greet", SymbolKind::Method),
        ]
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Inherits && e.target_name == "INamed")
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Imports && e.target_name == "System")
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "FormatName" && !c.is_member_call)
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "GetName" && c.is_member_call)
    );
}
