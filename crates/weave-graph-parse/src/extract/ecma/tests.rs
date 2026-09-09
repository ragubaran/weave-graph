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
