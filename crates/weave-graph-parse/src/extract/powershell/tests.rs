use crate::language::Language;
use crate::model::SymbolKind;
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::PowerShell)
        .unwrap()
        .parse("a.ps1", source)
        .unwrap()
}

#[test]
fn extracts_functions_and_calls() {
    let file = parse(
        "function Get-Greeting {\n    param($Name)\n    Format-Name $Name\n}\n\nfunction Format-Name {\n    param($Name)\n    return $Name\n}\n\nGet-Greeting -Name \"World\"\n",
    );
    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("Get-Greeting", SymbolKind::Function),
            ("Format-Name", SymbolKind::Function)
        ]
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "Format-Name" && !c.is_member_call)
    );
}
