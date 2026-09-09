use std::path::Path;

use weave_graph_parse::{ProjectIndex, SymbolKind, parse_file};

#[test]
fn r_extraction_extracts_functions_calls_and_classes() {
    let source = r#"
library(readr)
require(ggplot2)

DataFrameModel <- setRefClass("DataFrameModel")

load_and_process <- function(filepath) {
  raw_data <- readr::read_csv(filepath)
  cleaned <- clean_data(raw_data)
  model <- fit_model(cleaned)
  result <- model$predict(cleaned)
  return(result)
}

clean_data <- function(df) {
  df
}

fit_model <- function(df) {
  df
}
"#;

    let parsed = parse_file(Path::new("src/analysis.R"), source)
        .unwrap()
        .unwrap();

    let sym_names: Vec<&str> = parsed.symbols.iter().map(|s| s.symbol.as_str()).collect();
    assert!(sym_names.contains(&"DataFrameModel"));
    assert!(sym_names.contains(&"load_and_process"));
    assert!(sym_names.contains(&"clean_data"));
    assert!(sym_names.contains(&"fit_model"));

    let calls: Vec<&str> = parsed
        .calls
        .iter()
        .filter(|c| c.caller_moniker == "src/analysis.R#load_and_process")
        .map(|c| c.callee_name.as_str())
        .collect();

    assert!(calls.contains(&"read_csv"));
    assert!(calls.contains(&"clean_data"));
    assert!(calls.contains(&"fit_model"));
    assert!(calls.contains(&"predict"));
}

#[test]
fn r_multi_file_project_resolves_calls() {
    let file_a = r#"
helper_calc <- function(val) {
  val * 2
}
"#;

    let file_b = r#"
main_routine <- function(input) {
  out <- helper_calc(input)
  return(out)
}
"#;

    let parsed_a = parse_file(Path::new("R/helper.R"), file_a)
        .unwrap()
        .unwrap();
    let parsed_b = parse_file(Path::new("R/main.R"), file_b).unwrap().unwrap();

    let mut project = ProjectIndex::new();
    project.add_file(&parsed_a);
    project.add_file(&parsed_b);

    let edges = project.resolve(&parsed_b);
    let helper_call = edges.iter().find(|e| {
        e.source_moniker == "R/main.R#main_routine" && e.target_moniker == "R/helper.R#helper_calc"
    });

    assert!(
        helper_call.is_some(),
        "main_routine should resolve exact call to helper_calc across R files"
    );
    assert_eq!(helper_call.unwrap().kind, "CALLS_EXACT");
}

#[test]
fn r_lambda_and_assignment_variations() {
    let source = r#"
# Standard assignment
f1 <- function(x) x + 1

# Equals assignment
f2 = function(y) y * 2

# Modern lambda syntax
f3 <- \(z) z^2
"#;

    let parsed = parse_file(Path::new("scripts/lambdas.r"), source)
        .unwrap()
        .unwrap();

    let symbols: Vec<(&str, SymbolKind)> = parsed
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();

    assert!(symbols.contains(&("f1", SymbolKind::Function)));
    assert!(symbols.contains(&("f2", SymbolKind::Function)));
    assert!(symbols.contains(&("f3", SymbolKind::Function)));
}

#[test]
fn r_extraction_missed_branches() {
    let source = r#"
library()
require()
setGeneric()
setClass()

"" <- function() {}
df$col <- 5
empty_assign <- ""

test_calls <- function() {
    pkg::func()
    df$predict()
}
"" <- 5
setClass("MyLocalClass")
setGeneric("MyLocalGen")
"#;
    let parsed = parse_file(Path::new("src/edge.R"), source)
        .unwrap()
        .unwrap();
    let calls: Vec<&str> = parsed
        .calls
        .iter()
        .map(|c| c.callee_name.as_str())
        .collect();
    let symbols: Vec<&str> = parsed.symbols.iter().map(|s| s.symbol.as_str()).collect();
    assert!(calls.contains(&"func"), "func not found");
    assert!(calls.contains(&"predict"), "predict not found");
    assert!(symbols.contains(&"MyLocalClass"), "MyLocalClass not found");
    assert!(symbols.contains(&"MyLocalGen"), "MyLocalGen not found");
}
