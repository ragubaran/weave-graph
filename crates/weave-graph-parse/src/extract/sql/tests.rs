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

#[test]
fn create_function_becomes_a_function_symbol() {
    let file = parse(
        "CREATE FUNCTION add_one(x INT) RETURNS INT AS $$ SELECT x + 1 $$ LANGUAGE SQL;\n\
         CREATE FUNCTION twice(x INT) RETURNS INT AS $$ SELECT x * 2 $$ LANGUAGE SQL;\n",
    );
    let functions: Vec<&str> = file
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Function)
        .map(|s| s.symbol.as_str())
        .collect();
    assert!(functions.contains(&"add_one"), "{functions:?}");
    assert!(functions.contains(&"twice"), "{functions:?}");
}

#[test]
fn create_schema_becomes_a_class_symbol_and_an_empty_name_is_skipped() {
    let file = parse("CREATE SCHEMA analytics;\nCREATE SCHEMA \"\";\n");
    let classes: Vec<&str> = file
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Class)
        .map(|s| s.symbol.as_str())
        .collect();
    assert_eq!(classes, vec!["analytics"]);
}

#[test]
fn quoted_identifiers_are_unquoted_in_symbols_and_edges() {
    let file = parse(
        "CREATE TABLE \"order item\" (id INT);\n\
         CREATE VIEW v AS SELECT id FROM \"order item\";\n",
    );
    assert!(
        file.symbols
            .iter()
            .any(|s| s.symbol == "order item" && s.kind == SymbolKind::Struct)
    );
    assert!(
        file.structural_edges
            .iter()
            .any(|e| e.target_name == "order item")
    );
}

#[test]
fn materialized_views_extract_like_plain_views() {
    let file = parse(
        "CREATE TABLE users (id INT);\n\
         CREATE MATERIALIZED VIEW user_cache AS SELECT id FROM users;\n",
    );
    assert!(
        file.symbols
            .iter()
            .any(|s| s.symbol == "user_cache" && s.kind == SymbolKind::Interface)
    );
}
