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
