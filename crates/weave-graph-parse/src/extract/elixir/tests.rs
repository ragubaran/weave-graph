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

#[test]
fn a_def_nested_inside_a_non_keyword_call_is_still_found() {
    // Exercises extract_call_node's `_ => walk(...)` fallback: `if ... do
    // ... end` is itself a plain call (target "if"), not one of the
    // recognized keyword forms, so it must be walked into rather than
    // skipped — a common Elixir pattern for conditional compilation.
    let source = r#"
defmodule Config do
  if Code.ensure_loaded?(Jason) do
    def conditional_fn(x), do: x
  end
end
"#;
    let file = parse(source);
    assert!(
        file.symbols
            .iter()
            .any(|s| s.symbol == "Config::conditional_fn"),
        "a def nested inside a non-keyword call must still be extracted: {:?}",
        file.symbols
    );
}

#[test]
fn non_behaviour_module_attribute_is_ignored() {
    // extract_unary_operator only special-cases `@behaviour`; any other
    // module attribute (@moduledoc, @doc, ...) must hit its early return
    // rather than being misread as a behaviour edge.
    let source = r#"
defmodule Documented do
  @moduledoc "hello"

  def run(x) do
    x
  end
end
"#;
    let file = parse(source);
    assert!(
        !file
            .structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Implements),
        "a @moduledoc attribute must not be read as a behaviour"
    );
}

#[test]
fn defimpl_without_a_for_clause_uses_the_bare_protocol_name() {
    // Covers extract_impl's for_target.is_empty() branch and
    // find_for_target's "keywords present but none named for" path.
    let source = r#"
defprotocol Sized do
  def size(x)
end

defimpl Sized, other: :ignored do
  def size(_), do: 0
end
"#;
    let file = parse(source);
    let names: Vec<&str> = file.symbols.iter().map(|s| s.symbol.as_str()).collect();
    assert!(
        names.contains(&"Sized"),
        "bare protocol name used as the impl name: {names:?}"
    );
}

#[test]
fn dynamic_module_and_protocol_names_are_skipped_not_misparsed() {
    // defmodule/defprotocol whose name comes from an expression
    // (Module.concat/1) rather than a literal alias node — extract_module
    // and extract_protocol must bail out cleanly (no alias child) rather
    // than panicking or fabricating a symbol.
    let source = r#"
defmodule Module.concat([Foo, Bar]) do
  def run(x), do: x
end

defprotocol Module.concat([Proto, Baz]) do
  def go(x)
end
"#;
    let file = parse(source);
    assert!(
        !file
            .symbols
            .iter()
            .any(|s| s.kind == SymbolKind::Class || s.kind == SymbolKind::Interface),
        "a dynamic module/protocol name must not produce a fabricated symbol: {:?}",
        file.symbols
    );
}

#[test]
fn def_with_a_default_argument_is_named_via_the_binary_operators_left_side() {
    // extract_fn_name's "binary_operator" branch: default-value (`\\`)
    // syntax puts the real call (the fn name + its other params) on the
    // left of a binary_operator node, not directly as the first arg.
    let source = r#"
defmodule Handlers do
  def greet(name \\ "World") do
    name
  end
end
"#;
    let file = parse(source);
    let names: Vec<&str> = file.symbols.iter().map(|s| s.symbol.as_str()).collect();
    assert!(
        names.contains(&"Handlers::greet"),
        "default-argument def must still be named via the binary_operator's left side: {names:?}"
    );
}
