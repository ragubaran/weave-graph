use crate::language::Language;
use crate::model::StructuralEdgeKind;
use crate::parser::SourceParser;

fn parse_ts(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::TypeScript)
        .unwrap()
        .parse("a.ts", source)
        .unwrap()
}

#[test]
fn typescript_extends_without_implements_records_an_inherits_edge() {
    let file = parse_ts("class Base {}\nclass Child extends Base {}\n");
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Inherits && e.target_name == "Base"),
        "extends_clause must produce an INHERITS reference, not IMPLEMENTS"
    );
}

#[test]
fn calling_through_an_arrow_iife_is_not_treated_as_a_named_call() {
    let file = parse_ts("function run() { return (x => x)(5); }\n");
    assert!(
        file.calls.is_empty(),
        "no static callee name exists to record"
    );
}

#[test]
fn tsx_file_with_jsx_syntax_parses_without_error() {
    // `.tsx` must use the LANGUAGE_TSX grammar, not plain TypeScript's —
    // the two exist separately upstream because JSX's `<Tag>` and
    // TypeScript's old-style `<Type>value` cast are ambiguous, so one
    // grammar can't parse both. Using the wrong one silently drops any
    // function whose body contains JSX from the whole symbol table.
    let mut parser = SourceParser::new(Language::Tsx).unwrap();
    let source = "function Foo() { return null; }\nfunction App() { return <Foo />; }\n";
    let file = parser.parse("app.tsx", source).unwrap();

    assert!(
        !parser.tree().unwrap().root_node().has_error(),
        "the TSX grammar must parse JSX syntax cleanly"
    );
    assert!(
        file.symbols.iter().any(|s| s.symbol == "App"),
        "App must still be indexed now that its JSX-containing body parses"
    );
    // The extractor itself has no jsx_* walking yet, so a <Foo /> usage still
    // produces no reference edge to Foo — a narrower, separate gap from the
    // parse-error data loss this test now confirms is fixed.
    assert!(
        file.calls.iter().all(|c| c.callee_name != "Foo")
            && file.structural_edges.iter().all(|e| e.target_name != "Foo"),
        "JSX component usage still isn't walked by collect_calls/walk"
    );
}
