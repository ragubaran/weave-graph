use crate::language::Language;
use crate::model::StructuralEdgeKind;
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Rust)
        .unwrap()
        .parse("a.rs", source)
        .unwrap()
}

#[test]
fn nested_modules_qualify_symbols_by_full_module_path() {
    let file = parse("mod outer { fn f() {} mod inner { fn g() {} } }");
    let names: Vec<&str> = file.symbols.iter().map(|s| s.symbol.as_str()).collect();
    assert_eq!(names, vec!["outer::f", "outer::inner::g"]);
}

#[test]
fn grouped_use_list_produces_one_import_per_leaf() {
    let file = parse("use std::{fmt, collections::HashMap};");
    let mut targets: Vec<&str> = file
        .structural_edges
        .iter()
        .map(|e| e.target_name.as_str())
        .collect();
    targets.sort_unstable();
    assert_eq!(targets, vec!["HashMap", "fmt"]);
    assert!(
        file.structural_edges
            .iter()
            .all(|e| e.kind == StructuralEdgeKind::Imports)
    );
}

#[test]
fn use_as_clause_imports_the_original_name_not_the_alias() {
    let file = parse("use std::collections::HashMap as Map;");
    assert_eq!(file.structural_edges[0].target_name, "HashMap");
}

#[test]
fn fully_qualified_call_resolves_to_its_final_segment() {
    let file = parse("fn caller() { std::cmp::max(1, 2); }");
    assert_eq!(file.calls.len(), 1);
    assert_eq!(file.calls[0].callee_name, "max");
    assert!(!file.calls[0].is_member_call);
}

#[test]
fn calling_through_a_parenthesized_closure_is_not_treated_as_a_named_call() {
    let file = parse("fn caller() { (|x: i32| x)(5); }");
    assert!(
        file.calls.is_empty(),
        "no static callee name exists to record"
    );
}
