use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Zig)
        .unwrap()
        .parse("a.zig", source)
        .unwrap()
}

#[test]
fn extracts_struct_methods_calls_and_import() {
    let file = parse(
        "const std = @import(\"std\");\n\nconst Greeter = struct {\n    name: []const u8,\n\n    pub fn greet(self: Greeter) []const u8 {\n        return formatName(self.name);\n    }\n};\n\npub fn formatName(name: []const u8) []const u8 {\n    return name;\n}\n",
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
            ("Greeter::greet", SymbolKind::Method),
            ("formatName", SymbolKind::Function)
        ]
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "formatName" && !c.is_member_call)
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Imports && e.target_name == "std")
    );
}
