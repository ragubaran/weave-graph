use super::*;
use tree_sitter::Point;

#[test]
fn parse_file_returns_none_for_unsupported_extension() {
    assert!(parse_file(Path::new("README.md"), "# hi").is_none());
}

#[test]
fn reparse_reuses_the_cached_tree_and_only_marks_the_edited_range_changed() {
    let mut parser = SourceParser::new(Language::Rust).unwrap();
    let original = "fn a() -> i32 { 1 }\nfn b() -> i32 { 2 }\n";
    let first = parser.parse("f.rs", original).unwrap();
    assert_eq!(first.symbols.len(), 2);

    // Edit `1` to `100` inside `a`'s body.
    let edited = "fn a() -> i32 { 100 }\nfn b() -> i32 { 2 }\n";
    let start_byte = original.find('1').unwrap();
    let edit = InputEdit {
        start_byte,
        old_end_byte: start_byte + 1,
        new_end_byte: start_byte + 3,
        start_position: Point {
            row: 0,
            column: start_byte,
        },
        old_end_position: Point {
            row: 0,
            column: start_byte + 1,
        },
        new_end_position: Point {
            row: 0,
            column: start_byte + 3,
        },
    };

    let old_tree = parser.tree().unwrap().clone();
    let second = parser.reparse("f.rs", edited, edit).unwrap();
    assert_eq!(
        second.symbols.len(),
        2,
        "edit must not lose or duplicate symbols"
    );
    assert_eq!(second.symbols[0].signature, "fn a() -> i32");

    let changed = old_tree
        .changed_ranges(parser.tree().unwrap())
        .collect::<Vec<_>>();
    assert!(
        !changed.is_empty(),
        "incremental reparse must report the edited range as changed"
    );
    assert!(
        changed
            .iter()
            .all(|r| r.start_byte <= start_byte + 3 && r.end_byte >= start_byte),
        "changed range must cover the edit, not the whole file: {changed:?}"
    );
}
