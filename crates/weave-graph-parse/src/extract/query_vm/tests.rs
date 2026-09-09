use crate::language::Language;
use crate::model::SymbolKind;
use crate::parser::SourceParser;

fn parse(language: Language, path: &str, source: &str) -> crate::model::ParsedFile {
    SourceParser::new(language)
        .unwrap()
        .parse(path, source)
        .unwrap()
}

/// Haskell's `function`/`signature` nodes match the `function` marker
/// and carry a `name` field, so symbols are found — but its call node
/// is `apply`, which `is_call_kind` misses, so no calls are recorded.
/// Documents the honest limit rather than hiding it.
#[test]
fn haskell_symbols_found_but_calls_are_a_known_miss() {
    let file = parse(
        Language::Haskell,
        "a.hs",
        "greet :: String -> String\ngreet name = formatName name\n\nformatName :: String -> String\nformatName name = name\n",
    );
    let names: Vec<&str> = file.symbols.iter().map(|s| s.symbol.as_str()).collect();
    assert!(names.contains(&"greet"));
    assert!(names.contains(&"formatName"));
    assert!(
        file.calls.is_empty(),
        "Haskell's `apply` call node isn't in the generic call-kind list — documented miss, not a crash"
    );
}

#[test]
fn universal_fallback_routes_and_extracts_via_query_vm() {
    let source = "addOne :: Int -> Int\naddOne x = x + 1\n";
    let file = parse(Language::Haskell, "Math.hs", source);
    assert!(file.symbols.iter().any(|s| s.symbol == "addOne"));
}

#[test]
fn query_vm_generic_heuristics_extract_assignment_functions_and_calls() {
    let source = "calc <- function(val) {\n  res <- helper(val)\n  return(res)\n}\n";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&Language::R.grammar())
        .expect("grammar");
    let tree = parser.parse(source, None).expect("parse");
    let file = super::extract(Language::R, tree.root_node(), source.as_bytes(), "calc.R");

    let sym = file.symbols.iter().find(|s| s.symbol == "calc");
    assert!(
        sym.is_some(),
        "Generic query_vm must extract assignment function 'calc'"
    );
    assert_eq!(sym.unwrap().kind, SymbolKind::Function);

    let has_call = file.calls.iter().any(|c| c.callee_name == "helper");
    assert!(has_call, "Generic query_vm must extract call to 'helper'");
}

#[test]
fn query_vm_json_extends_and_main_imports() {
    let source = r#"{
      "extends": "tsconfig.base.json",
      "main": "index.js",
      "name": "my-pkg",
      "": "empty key"
    }"#;
    let file = parse(Language::Json, "package.json", source);
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.target_name == "tsconfig.base.json")
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.target_name == "index.js")
    );
    assert!(file.symbols.iter().any(|s| s.symbol == "name"));
    assert!(!file.symbols.iter().any(|s| s.symbol.is_empty()));
}

#[test]
fn query_vm_yaml_depends_on_and_include_role() {
    let source = r#"
depends_on:
  - serviceA
include_role:
  - roleB
extends:
  - base.yaml
"": empty
    "#;
    let file = parse(Language::Yaml, "docker-compose.yml", source);
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.target_name == "serviceA")
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.target_name == "roleB")
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.target_name == "base.yaml")
    );
    assert!(!file.symbols.iter().any(|s| s.symbol.is_empty()));
}

#[test]
fn query_vm_toml_and_properties_empty_keys() {
    let source_toml = "[\"\"]\n\"\" = 1\n[table]\nkey = 2\n";
    let file_toml = parse(Language::Toml, "config.toml", source_toml);
    assert!(file_toml.symbols.iter().any(|s| s.symbol == "table"));
    assert!(file_toml.symbols.iter().any(|s| s.symbol == "key"));
    assert!(!file_toml.symbols.iter().any(|s| s.symbol.is_empty()));

    let source_prop = "=empty\nvalid=ok\n";
    let file_prop = parse(Language::Properties, ".env", source_prop);
    assert!(file_prop.symbols.iter().any(|s| s.symbol == "valid"));
    assert!(!file_prop.symbols.iter().any(|s| s.symbol.is_empty()));
}

#[test]
fn query_vm_generic_heuristics_last_identifier_fallback() {
    let source = r#"
function myFunction() {
    obj.methodCall(arg);
    plainCall();
}
"#;
    let language = Language::JavaScript;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language.grammar()).unwrap();
    let tree = parser.parse(source, None).unwrap();
    // Use Haskell language so it uses the generic query_vm.rs fallback
    let file = super::extract(
        Language::Haskell,
        tree.root_node(),
        source.as_bytes(),
        "test.js",
    );

    let calls: Vec<&str> = file.calls.iter().map(|c| c.callee_name.as_str()).collect();
    assert!(
        calls.contains(&"methodCall"),
        "Should find methodCall via last_identifier_like_descendant"
    );
    assert!(calls.contains(&"plainCall"), "Should find plainCall");
}
