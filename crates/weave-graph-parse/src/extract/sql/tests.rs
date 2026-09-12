use crate::language::Language;
use crate::model::{StructuralEdgeKind, SymbolKind};
use crate::parser::SourceParser;

fn parse(source: &str) -> crate::model::ParsedFile {
    SourceParser::new(Language::Sql)
        .unwrap()
        .parse("schema.sql", source)
        .unwrap()
}

#[test]
fn extracts_tables_and_foreign_keys() {
    let file = parse(
        "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(50));\n\
         CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));\n",
    );
    let names: Vec<(&str, SymbolKind)> = file
        .symbols
        .iter()
        .map(|s| (s.symbol.as_str(), s.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("users", SymbolKind::Struct),
            ("orders", SymbolKind::Struct),
        ]
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.kind == StructuralEdgeKind::Imports && e.target_name == "users")
    );
}

#[test]
fn extracts_views_and_relations() {
    let file = parse(
        "CREATE TABLE users (id INT PRIMARY KEY);\n\
         CREATE VIEW active_users AS SELECT id FROM users;\n",
    );
    assert!(
        file.symbols
            .iter()
            .any(|s| s.symbol == "active_users" && s.kind == SymbolKind::Interface)
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.source_moniker == "schema.sql#active_users" && e.target_name == "users")
    );
}

#[test]
fn extracts_triggers_and_calls() {
    let file = parse(
        "CREATE TABLE audit_log (id INT);\n\
         CREATE TRIGGER on_audit AFTER INSERT ON audit_log FOR EACH ROW EXECUTE FUNCTION log_audit();\n",
    );
    assert!(
        file.symbols
            .iter()
            .any(|s| s.symbol == "on_audit" && s.kind == SymbolKind::Impl)
    );
    assert!(
        file.calls
            .iter()
            .any(|c| c.caller_moniker == "schema.sql#on_audit" && c.callee_name == "log_audit")
    );
}

#[test]
fn extracts_schema_qualified_table_names() {
    let file = parse("CREATE TABLE myschema.mytable (id INT);\n");
    assert!(
        file.symbols
            .iter()
            .any(|s| s.symbol == "myschema.mytable" && s.kind == SymbolKind::Struct)
    );
}

#[test]
fn create_index_with_a_name_is_a_symbol_linked_to_its_table() {
    let file = parse("CREATE TABLE t (col INT);\nCREATE INDEX idx_t_col ON t(col);\n");
    assert!(
        file.symbols
            .iter()
            .any(|s| s.symbol == "idx_t_col" && s.kind == SymbolKind::Impl)
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.source_moniker == "schema.sql#idx_t_col" && e.target_name == "t")
    );
}

#[test]
fn create_index_with_no_name_is_skipped() {
    // `CREATE INDEX ON t(col)` (no index name) leaves `index_name` empty —
    // `extract_create_index` must return without pushing a symbol, not
    // panic or push a blank one.
    let file = parse("CREATE TABLE t (col INT);\nCREATE INDEX ON t(col);\n");
    assert!(!file.symbols.iter().any(|s| s.kind == SymbolKind::Impl));
}

#[test]
fn alter_table_records_a_foreign_key_reference() {
    let file = parse(
        "CREATE TABLE users (id INT);\n\
         ALTER TABLE orders ADD CONSTRAINT fk FOREIGN KEY (user_id) REFERENCES users(id);\n",
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.source_moniker == "schema.sql#orders" && e.target_name == "users")
    );
}

#[test]
fn create_type_without_an_as_clause_is_still_a_symbol() {
    let file = parse("CREATE TYPE status;\n");
    assert!(
        file.symbols
            .iter()
            .any(|s| s.symbol == "status" && s.kind == SymbolKind::Struct)
    );
}

#[test]
fn unquote_strips_matching_delimiters_of_every_supported_kind() {
    assert_eq!(super::unquote("\"double\""), "double");
    assert_eq!(super::unquote("`backtick`"), "backtick");
    assert_eq!(super::unquote("'single'"), "single");
    assert_eq!(super::unquote("[bracket]"), "bracket");
    assert_eq!(super::unquote("bare"), "bare");
}

#[test]
fn unquote_leaves_a_lone_delimiter_character_untouched() {
    // A single `"` both starts_with and ends_with '"' at len 1 — stripping
    // one char from each end would panic on an empty slice, so this
    // degenerate case must fall through unchanged instead.
    assert_eq!(super::unquote("\""), "\"");
}
