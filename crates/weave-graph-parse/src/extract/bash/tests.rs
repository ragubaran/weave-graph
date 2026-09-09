use crate::language::Language;
use crate::model::SymbolKind;
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Bash)
        .unwrap()
        .parse("a.sh", source)
        .unwrap()
}

#[test]
fn extracts_functions_and_calls() {
    let file = parse(
        "greet() {\n    format_name \"$name\"\n}\n\nformat_name() {\n    echo \"$1\"\n}\n\ngreet\n",
    );
    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("greet", SymbolKind::Function),
            ("format_name", SymbolKind::Function)
        ]
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "format_name" && !c.is_member_call)
    );
    assert!(file.calls.iter().any(|c| c.callee_name == "echo"));
}
