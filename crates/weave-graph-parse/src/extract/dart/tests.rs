use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Dart)
        .unwrap()
        .parse("sample.dart", source)
        .unwrap()
}

#[test]
fn extracts_classes_inheritance_and_methods() {
    let source = r#"
import 'package:flutter/material.dart';

abstract class Greeter {
  String greet();
}

class LoudGreeter extends Greeter with Logging implements Printable {
  String name;

  LoudGreeter(this.name);

  String greet() {
    return formatName(name);
  }
}

String formatName(String name) {
  return name.trim();
}
"#;
    let file = parse(source);

    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();

    assert!(names.contains(&("Greeter", SymbolKind::Class)));
    assert!(names.contains(&("Greeter::greet", SymbolKind::Method)));
    assert!(names.contains(&("LoudGreeter", SymbolKind::Class)));
    assert!(names.contains(&("LoudGreeter::greet", SymbolKind::Method)));
    assert!(names.contains(&("formatName", SymbolKind::Function)));

    // Inherits Greeter and Logging
    assert!(file.structural_edges.iter().any(|e| {
        e.source_moniker == "sample.dart#LoudGreeter"
            && e.target_name == "Greeter"
            && e.kind == StructuralEdgeKind::Inherits
    }));
    assert!(file.structural_edges.iter().any(|e| {
        e.source_moniker == "sample.dart#LoudGreeter"
            && e.target_name == "Logging"
            && e.kind == StructuralEdgeKind::Inherits
    }));

    // Implements Printable
    assert!(file.structural_edges.iter().any(|e| {
        e.source_moniker == "sample.dart#LoudGreeter"
            && e.target_name == "Printable"
            && e.kind == StructuralEdgeKind::Implements
    }));

    // Import package
    assert!(file.structural_edges.iter().any(|e| {
        e.kind == StructuralEdgeKind::Imports && e.target_name == "package:flutter/material.dart"
    }));

    // Calls formatName and trim
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "formatName" && !c.is_member_call)
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "trim" && c.is_member_call)
    );
}

#[test]
fn extracts_mixins_and_extensions() {
    let source = r#"
mixin Swimmer on Animal {
  void swim() {}
}

extension StringOps on String {
  int toNum() => int.parse(this);
}
"#;
    let file = parse(source);

    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();

    assert!(names.contains(&("Swimmer", SymbolKind::Interface)));
    assert!(names.contains(&("Swimmer::swim", SymbolKind::Method)));
    assert!(names.contains(&("StringOps", SymbolKind::Impl)));
    assert!(names.contains(&("StringOps::toNum", SymbolKind::Method)));

    // Mixin inherits Animal
    assert!(file.structural_edges.iter().any(|e| {
        e.source_moniker == "sample.dart#Swimmer"
            && e.target_name == "Animal"
            && e.kind == StructuralEdgeKind::Inherits
    }));

    // Extension implements String
    assert!(file.structural_edges.iter().any(|e| {
        e.source_moniker == "sample.dart#StringOps"
            && e.target_name == "String"
            && e.kind == StructuralEdgeKind::Implements
    }));
}
