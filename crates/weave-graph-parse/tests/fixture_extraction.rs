use std::path::Path;

use weave_graph_parse::{SymbolKind, parse_file};

/// `impl.md` M1.2's required check: symbol extraction matches a
/// hand-checked fixture file per language. Every `(symbol, kind, line_start,
/// line_end)` tuple below was counted by hand against the fixture file
/// next to this test, not derived from the extractor's own output.
fn parse_fixture(name: &str) -> weave_graph_parse::ParsedFile {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let source = std::fs::read_to_string(&path).unwrap();
    parse_file(&path, &source).unwrap().unwrap()
}

fn symbol_tuples(file: &weave_graph_parse::ParsedFile) -> Vec<(&str, SymbolKind, u32, u32)> {
    file.symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind, s.line_start, s.line_end))
        .collect()
}

#[test]
fn rust_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.rs");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Greeter", SymbolKind::Struct, 3, 5),
            ("Greeter::new", SymbolKind::Method, 8, 10),
            ("Greeter::greet", SymbolKind::Method, 12, 14),
            ("Greeter::bye", SymbolKind::Method, 22, 24),
            ("format_name", SymbolKind::Function, 27, 29),
        ]
    );
    assert_eq!(
        file.symbols[1].signature,
        "pub fn new(name: String) -> Self"
    );

    assert!(
        file.structural_edges.iter().any(|e| e.kind
            == weave_graph_parse::StructuralEdgeKind::Implements
            && e.target_name == "Farewell"),
        "impl Farewell for Greeter must record an IMPLEMENTS reference to Farewell"
    );
}

#[test]
fn python_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.py");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Greeter", SymbolKind::Class, 4, 9),
            ("Greeter::__init__", SymbolKind::Method, 5, 6),
            ("Greeter::greet", SymbolKind::Method, 8, 9),
            ("LoudGreeter", SymbolKind::Class, 12, 14),
            ("LoudGreeter::greet", SymbolKind::Method, 13, 14),
            ("format_name", SymbolKind::Function, 17, 18),
        ]
    );

    assert!(
        file.structural_edges.iter().any(|e| e.kind
            == weave_graph_parse::StructuralEdgeKind::Inherits
            && e.target_name == "Greeter"),
        "class LoudGreeter(Greeter) must record an INHERITS reference to Greeter"
    );
}

#[test]
fn javascript_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.js");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Greeter", SymbolKind::Class, 3, 11),
            ("Greeter::constructor", SymbolKind::Method, 4, 6),
            ("Greeter::greet", SymbolKind::Method, 8, 10),
            ("LoudGreeter", SymbolKind::Class, 13, 17),
            ("LoudGreeter::greet", SymbolKind::Method, 14, 16),
            ("formatName", SymbolKind::Function, 19, 21),
        ]
    );

    assert!(
        file.structural_edges.iter().any(|e| e.kind
            == weave_graph_parse::StructuralEdgeKind::Inherits
            && e.target_name == "Greeter"),
        "class LoudGreeter extends Greeter must record an INHERITS reference to Greeter"
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == weave_graph_parse::StructuralEdgeKind::Imports
                && e.target_name == "./strings"),
        "import must record the module path"
    );
}

#[test]
fn typescript_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.ts");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Named", SymbolKind::Interface, 3, 5),
            ("Named::getName", SymbolKind::Method, 4, 4),
            ("Greeter", SymbolKind::Class, 7, 17),
            ("Greeter::constructor", SymbolKind::Method, 8, 8),
            ("Greeter::getName", SymbolKind::Method, 10, 12),
            ("Greeter::greet", SymbolKind::Method, 14, 16),
            ("formatName", SymbolKind::Function, 19, 21),
        ]
    );

    assert!(
        file.structural_edges.iter().any(|e| e.kind
            == weave_graph_parse::StructuralEdgeKind::Implements
            && e.target_name == "Named"),
        "class Greeter implements Named must record an IMPLEMENTS reference to Named"
    );
}

/// M1.2b (`impl.md` §0a): same hand-checked standard as M1.2's four
/// languages above, extended to the 14 additional languages.
#[test]
fn go_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.go");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Greeter", SymbolKind::Struct, 5, 7),
            ("Greeter::Greet", SymbolKind::Method, 9, 11),
            ("formatName", SymbolKind::Function, 13, 16),
            ("Named", SymbolKind::Interface, 18, 20),
        ]
    );
}

#[test]
fn java_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.java");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Named", SymbolKind::Interface, 3, 5),
            ("Named::getName", SymbolKind::Method, 4, 4),
            ("Greeter", SymbolKind::Class, 7, 17),
            ("Greeter::getName", SymbolKind::Method, 10, 12),
            ("Greeter::greet", SymbolKind::Method, 14, 16),
            ("LoudGreeter", SymbolKind::Class, 19, 20),
        ]
    );
}

#[test]
fn c_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.c");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Point", SymbolKind::Struct, 3, 6),
            ("add", SymbolKind::Function, 8, 10),
            ("helper", SymbolKind::Function, 12, 14),
        ]
    );
}

#[test]
fn cpp_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.cpp");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("add", SymbolKind::Function, 3, 5),
            ("app::Point", SymbolKind::Struct, 9, 12),
            ("app::Base", SymbolKind::Class, 14, 17),
            ("app::Greeter", SymbolKind::Class, 19, 31),
            ("app::Greeter::greet", SymbolKind::Method, 21, 24),
            ("app::Greeter::helper", SymbolKind::Method, 25, 27),
            ("app::Greeter::getPtr", SymbolKind::Method, 28, 30),
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn csharp_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.cs");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("App::INamed", SymbolKind::Interface, 4, 6),
            ("App::INamed::GetName", SymbolKind::Method, 5, 5),
            ("App::Greeter", SymbolKind::Class, 8, 16),
            ("App::Greeter::GetName", SymbolKind::Method, 9, 11),
            ("App::Greeter::Greet", SymbolKind::Method, 13, 15),
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn kotlin_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.kt");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Named", SymbolKind::Interface, 3, 5),
            ("Named::getName", SymbolKind::Method, 4, 4),
            ("Greeter", SymbolKind::Class, 7, 15),
            ("Greeter::getName", SymbolKind::Method, 8, 10),
            ("Greeter::greet", SymbolKind::Method, 12, 14),
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn swift_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.swift");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Named", SymbolKind::Interface, 1, 3),
            ("Named::getName", SymbolKind::Method, 2, 2),
            ("Greeter", SymbolKind::Class, 5, 13),
            ("Greeter::getName", SymbolKind::Method, 6, 8),
            ("Greeter::greet", SymbolKind::Method, 10, 12),
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn scala_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.scala");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Named", SymbolKind::Interface, 3, 5),
            ("Named::getName", SymbolKind::Method, 4, 4),
            ("Greeter", SymbolKind::Class, 7, 15),
            ("Greeter::getName", SymbolKind::Method, 8, 10),
            ("Greeter::greet", SymbolKind::Method, 12, 14),
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn zig_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.zig");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Greeter", SymbolKind::Struct, 3, 9),
            ("Greeter::greet", SymbolKind::Method, 6, 8),
            ("formatName", SymbolKind::Function, 11, 13),
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn ruby_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.rb");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Greeter", SymbolKind::Class, 1, 9),
            ("Greeter::initialize", SymbolKind::Method, 2, 4),
            ("Greeter::greet", SymbolKind::Method, 6, 8),
            ("LoudGreeter", SymbolKind::Class, 11, 12),
            ("format_name", SymbolKind::Function, 14, 16),
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn php_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.php");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Named", SymbolKind::Interface, 5, 7),
            ("Named::getName", SymbolKind::Method, 6, 6),
            ("Greeter", SymbolKind::Class, 9, 17),
            ("Greeter::getName", SymbolKind::Method, 10, 12),
            ("Greeter::greet", SymbolKind::Method, 14, 16),
            ("LoudGreeter", SymbolKind::Class, 19, 20),
            ("formatName", SymbolKind::Function, 22, 24),
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn bash_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.sh");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("greet", SymbolKind::Function, 1, 3),
            ("format_name", SymbolKind::Function, 5, 7)
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn powershell_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.ps1");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Get-Greeting", SymbolKind::Function, 1, 4),
            ("Format-Name", SymbolKind::Function, 6, 9)
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn lua_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.lua");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("formatName", SymbolKind::Function, 3, 5),
            ("greet", SymbolKind::Function, 7, 9)
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn sql_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.sql");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("users", SymbolKind::Struct, 1, 5),
            ("orders", SymbolKind::Struct, 7, 11),
            ("idx_orders_user", SymbolKind::Impl, 13, 13),
            ("user_orders", SymbolKind::Interface, 15, 18),
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn dart_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.dart");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Greeter", SymbolKind::Class, 1, 3),
            ("Greeter::greet", SymbolKind::Method, 2, 2),
            ("LoudGreeter", SymbolKind::Class, 5, 13),
            ("LoudGreeter::LoudGreeter", SymbolKind::Method, 8, 8),
            ("LoudGreeter::greet", SymbolKind::Method, 10, 12),
            ("formatName", SymbolKind::Function, 15, 17),
        ]
    );
    assert!(file.structural_edges.iter().any(|e| {
        e.source_moniker.ends_with("#LoudGreeter")
            && e.target_name == "Greeter"
            && e.kind == weave_graph_parse::StructuralEdgeKind::Inherits
    }));
}

#[test]
#[cfg(feature = "lang-extended")]
fn elixir_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.ex");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("Greeter", SymbolKind::Class, 1, 9),
            ("Greeter::greet", SymbolKind::Method, 2, 4),
            ("Greeter::format_name", SymbolKind::Method, 6, 8),
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn html_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.html");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("main-header", SymbolKind::Struct, 8, 8),
            ("title", SymbolKind::Struct, 9, 9),
            ("user-card", SymbolKind::Class, 11, 11),
            ("user-1", SymbolKind::Struct, 11, 11),
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn css_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.css");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("main-header", SymbolKind::Struct, 3, 5),
            ("nav", SymbolKind::Class, 7, 9),
            ("fadeIn", SymbolKind::Function, 11, 14),
        ]
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn r_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.R");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("ModelConfig", SymbolKind::Class, 3, 3),
            ("calculate_score", SymbolKind::Function, 5, 8),
            ("format_output", SymbolKind::Function, 10, 12),
        ]
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == weave_graph_parse::StructuralEdgeKind::Imports
                && e.target_name == "stats")
    );
}

#[test]
#[cfg(feature = "lang-extended")]
fn haskell_fixture_symbols_match_hand_count() {
    let file = parse_fixture("sample.hs");
    assert_eq!(
        symbol_tuples(&file),
        vec![
            ("factorial", SymbolKind::Function, 5, 5),
            ("factorial", SymbolKind::Function, 6, 6),
            ("distance", SymbolKind::Function, 16, 19),
        ]
    );
}
