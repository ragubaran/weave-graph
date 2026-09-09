use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse_r(path: &str, source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::R)
        .unwrap()
        .parse(path, source)
        .unwrap()
}

#[test]
fn extracts_r_functions_classes_and_calls() {
    let code = r#"
library(dplyr)
require("ggplot2")

Person <- setRefClass("Person")

calculate_stats <- function(x) {
  s <- base::sum(x)
  m <- mean(x)
  model <- fit_model(x)
  pred <- model$predict(x)
  return(pred)
}

add_values = function(a, b) {
  a + b
}

scalar_var <- 42
"#;

    let file = parse_r("analysis.R", code);

    // Symbols
    let symbols: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();

    assert!(symbols.contains(&("Person", SymbolKind::Class)));
    assert!(symbols.contains(&("calculate_stats", SymbolKind::Function)));
    assert!(symbols.contains(&("add_values", SymbolKind::Function)));
    assert!(!symbols.iter().any(|(name, _)| *name == "scalar_var"));

    // Imports
    let imports: Vec<(&str, &str)> = file
        .structural_edges
        .iter()
        .filter(|e| e.kind == StructuralEdgeKind::Imports)
        .map(|e| (e.source_moniker.as_str(), e.target_name.as_str()))
        .collect();

    assert!(imports.contains(&("analysis.R#<module>", "dplyr")));
    assert!(imports.contains(&("analysis.R#<module>", "ggplot2")));

    // Calls within calculate_stats
    let calls: Vec<(&str, bool)> = file
        .calls
        .iter()
        .filter(|c| c.caller_moniker == "analysis.R#calculate_stats")
        .map(|c| (c.callee_name.as_str(), c.is_member_call))
        .collect();

    assert!(calls.contains(&("sum", false)));
    assert!(calls.contains(&("mean", false)));
    assert!(calls.contains(&("fit_model", false)));
    assert!(calls.contains(&("predict", true)));
}
