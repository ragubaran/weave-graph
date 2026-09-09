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
