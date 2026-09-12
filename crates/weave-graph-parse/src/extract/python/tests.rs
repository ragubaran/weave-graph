use crate::language::Language;
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Python)
        .unwrap()
        .parse("a.py", source)
        .unwrap()
}

#[test]
fn from_import_with_multiple_names_produces_one_edge_per_name() {
    let file = parse("from os import path, sep\n");
    let mut targets: Vec<&str> = file
        .structural_edges
        .iter()
        .map(|e| e.target_name.as_str())
        .collect();
    targets.sort_unstable();
    assert_eq!(targets, vec!["path", "sep"]);
}

#[test]
fn dotted_import_uses_the_final_segment() {
    let file = parse("import os.path\n");
    assert_eq!(file.structural_edges[0].target_name, "path");
}

#[test]
fn aliased_import_uses_the_original_name_not_the_alias() {
    let file = parse("from collections import OrderedDict as OD\n");
    assert_eq!(file.structural_edges[0].target_name, "OrderedDict");
}

#[test]
fn bare_single_segment_import_resolves_via_plain_identifier() {
    let file = parse("import os\n");
    assert_eq!(file.structural_edges[0].target_name, "os");
}

#[test]
fn star_import_produces_no_structural_edge() {
    let file = parse("from os import *\n");
    assert!(file.structural_edges.is_empty());
}

#[test]
fn call_through_a_subscript_is_not_recorded_as_a_named_call() {
    let file = parse("def f():\n    handlers[0]()\n");
    assert!(file.calls.is_empty());
}
