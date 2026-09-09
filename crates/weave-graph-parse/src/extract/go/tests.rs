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
