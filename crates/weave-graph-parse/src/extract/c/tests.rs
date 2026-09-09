use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::C)
        .unwrap()
        .parse("a.c", source)
        .unwrap()
}

#[test]
fn extracts_struct_functions_calls_and_includes() {
    let file = parse(
        "#include <stdio.h>\nstruct Point { int x; int y; };\nint add(int a, int b) { return helper(a, b); }\nint helper(int a, int b) { return a + b; }\n",
    );
    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("Point", SymbolKind::Struct),
            ("add", SymbolKind::Function),
            ("helper", SymbolKind::Function)
        ]
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "helper" && !c.is_member_call)
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Imports && e.target_name == "stdio.h")
    );
}
