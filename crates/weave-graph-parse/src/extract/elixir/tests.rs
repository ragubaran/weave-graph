use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Elixir)
        .unwrap()
        .parse("sample.ex", source)
        .unwrap()
}

#[test]
fn extracts_modules_functions_and_calls() {
    let source = r#"
defmodule MyApp.Greeter do
  @behaviour MyApp.GreeterBehaviour
  alias MyApp.Formatter

  def greet(name) do
    Formatter.format(name)
  end

  defp helper(x) do
    do_something(x)
  end
end
"#;
    let file = parse(source);

    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();

    assert!(names.contains(&("MyApp.Greeter", SymbolKind::Class)));
    assert!(names.contains(&("MyApp.Greeter::greet", SymbolKind::Method)));
    assert!(names.contains(&("MyApp.Greeter::helper", SymbolKind::Method)));

    // Behaviour implementation
    assert!(file.structural_edges.iter().any(|e| {
        e.target_name == "MyApp.GreeterBehaviour" && e.kind == StructuralEdgeKind::Implements
    }));

    // Alias import
    assert!(
        file.structural_edges.iter().any(|e| {
            e.target_name == "MyApp.Formatter" && e.kind == StructuralEdgeKind::Imports
        })
    );

    // Calls Formatter.format and do_something
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "format" && c.is_member_call)
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.callee_name == "do_something" && !c.is_member_call)
    );
}

#[test]
fn extracts_protocols_and_implementations() {
    let source = r#"
defprotocol Printable do
  def print(data)
end

defimpl Printable, for: MyApp.User do
  def print(user) do
    user.name
  end
end
"#;
    let file = parse(source);

    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();

    assert!(names.contains(&("Printable", SymbolKind::Interface)));
    assert!(names.contains(&("Printable::print", SymbolKind::Method)));
    assert!(names.contains(&("Printable.MyApp.User", SymbolKind::Impl)));

    // Defimpl implements Printable
    assert!(file.structural_edges.iter().any(|e| {
        e.source_moniker == "sample.ex#Printable.MyApp.User"
            && e.target_name == "Printable"
            && e.kind == StructuralEdgeKind::Implements
    }));
}
