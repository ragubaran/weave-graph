use crate::language::Language;
use crate::model::SymbolKind;
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Lua)
        .unwrap()
        .parse("a.lua", source)
        .unwrap()
}

#[test]
fn extracts_functions_and_calls() {
    let file = parse(
        "require(\"json\")\n\nlocal function formatName(name)\n    return name:upper()\nend\n\nfunction greet(name)\n    return formatName(name)\nend\n\ngreet(\"world\")\n",
    );
    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("formatName", SymbolKind::Function),
            ("greet", SymbolKind::Function)
        ]
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "upper" && c.is_member_call)
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "formatName" && !c.is_member_call)
    );
}
