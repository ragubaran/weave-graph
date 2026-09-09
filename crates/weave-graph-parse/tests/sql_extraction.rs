use std::path::Path;

use weave_graph_parse::{StructuralEdgeKind, SymbolKind, parse_file};

#[test]
fn sql_table_extraction() {
    let sql = r#"
        CREATE TABLE users (
            id INT PRIMARY KEY,
            username VARCHAR(50) NOT NULL,
            email VARCHAR(255)
        );

        CREATE TABLE orders (
            id INT PRIMARY KEY,
            user_id INT REFERENCES users(id),
            total_cents INT NOT NULL
        );
    "#;

    let parsed = parse_file(Path::new("schema.sql"), sql)
        .unwrap()
        .expect("must parse SQL");

    assert_eq!(parsed.symbols.len(), 2);
    let users_sym = parsed.symbols.iter().find(|s| s.symbol == "users").unwrap();
    assert_eq!(users_sym.kind, SymbolKind::Struct);
    assert_eq!(users_sym.moniker, "schema.sql#users");

    let orders_sym = parsed
        .symbols
        .iter()
        .find(|s| s.symbol == "orders")
        .unwrap();
    assert_eq!(orders_sym.kind, SymbolKind::Struct);
    assert_eq!(orders_sym.moniker, "schema.sql#orders");

    // Check foreign key reference from orders to users
    let fk_edge = parsed
        .structural_edges
        .iter()
        .find(|e| e.source_moniker == "schema.sql#orders" && e.target_name == "users");
    assert!(
        fk_edge.is_some(),
        "orders must reference users via foreign key"
    );
    assert_eq!(fk_edge.unwrap().kind, StructuralEdgeKind::Imports);
}

#[test]
fn sql_view_and_index_extraction() {
    let sql = r#"
        CREATE TABLE products (
            id INT PRIMARY KEY,
            name VARCHAR(100),
            price DECIMAL(10, 2)
        );

        CREATE INDEX idx_products_price ON products(price);

        CREATE VIEW cheap_products AS
        SELECT id, name
        FROM products
        WHERE price < 20.0;
    "#;

    let parsed = parse_file(Path::new("views.sql"), sql)
        .unwrap()
        .expect("must parse SQL");

    let products = parsed.symbols.iter().find(|s| s.symbol == "products");
    let idx = parsed
        .symbols
        .iter()
        .find(|s| s.symbol == "idx_products_price");
    let view = parsed.symbols.iter().find(|s| s.symbol == "cheap_products");

    assert!(products.is_some(), "must extract products table");
    assert_eq!(products.unwrap().kind, SymbolKind::Struct);

    assert!(idx.is_some(), "must extract index");
    assert_eq!(idx.unwrap().kind, SymbolKind::Impl);

    assert!(view.is_some(), "must extract view");
    assert_eq!(view.unwrap().kind, SymbolKind::Interface);

    // Index points to products table
    let idx_edge = parsed.structural_edges.iter().find(|e| {
        e.source_moniker == "views.sql#idx_products_price" && e.target_name == "products"
    });
    assert!(idx_edge.is_some(), "index must reference products table");

    // View queries products table
    let view_edge = parsed
        .structural_edges
        .iter()
        .find(|e| e.source_moniker == "views.sql#cheap_products" && e.target_name == "products");
    assert!(view_edge.is_some(), "view must reference products table");
}

#[test]
fn sql_alter_table_foreign_key() {
    let sql = r#"
        CREATE TABLE authors (
            id INT PRIMARY KEY,
            name VARCHAR(100)
        );

        CREATE TABLE books (
            id INT PRIMARY KEY,
            author_id INT
        );

        ALTER TABLE books ADD CONSTRAINT fk_author FOREIGN KEY (author_id) REFERENCES authors(id);
    "#;

    let parsed = parse_file(Path::new("migrations/001_books.sql"), sql)
        .unwrap()
        .expect("must parse SQL");

    let fk_edge = parsed.structural_edges.iter().find(|e| {
        e.source_moniker == "migrations/001_books.sql#books" && e.target_name == "authors"
    });
    assert!(
        fk_edge.is_some(),
        "ALTER TABLE must add edge from books to authors"
    );
}

#[test]
fn sql_triggers_and_functions() {
    let sql = r#"
        CREATE FUNCTION log_audit() RETURNS TRIGGER AS $$
        BEGIN
            RETURN NEW;
        END;
        $$ LANGUAGE plpgsql;

        CREATE TABLE audit_log (
            id INT PRIMARY KEY
        );

        CREATE TRIGGER on_audit_insert
        AFTER INSERT ON audit_log
        FOR EACH ROW EXECUTE FUNCTION log_audit();
    "#;

    let parsed = parse_file(Path::new("triggers.sql"), sql)
        .unwrap()
        .expect("must parse SQL");

    let func = parsed.symbols.iter().find(|s| s.symbol == "log_audit");
    assert!(func.is_some(), "must extract log_audit function");
    assert_eq!(func.unwrap().kind, SymbolKind::Function);

    let trig = parsed
        .symbols
        .iter()
        .find(|s| s.symbol == "on_audit_insert");
    assert!(trig.is_some(), "must extract on_audit_insert trigger");
    assert_eq!(trig.unwrap().kind, SymbolKind::Impl);

    // Trigger references audit_log table
    let trig_table_edge = parsed.structural_edges.iter().find(|e| {
        e.source_moniker == "triggers.sql#on_audit_insert" && e.target_name == "audit_log"
    });
    assert!(
        trig_table_edge.is_some(),
        "trigger must reference audit_log"
    );

    // Trigger calls log_audit function
    let trig_call = parsed.calls.iter().find(|c| {
        c.caller_moniker == "triggers.sql#on_audit_insert" && c.callee_name == "log_audit"
    });
    assert!(trig_call.is_some(), "trigger must call log_audit function");
}

#[test]
fn sql_schemas_and_types() {
    let sql = r#"
        CREATE SCHEMA analytics;

        CREATE TYPE order_status AS ENUM ('pending', 'shipped', 'delivered');
    "#;

    let parsed = parse_file(Path::new("types.sql"), sql)
        .unwrap()
        .expect("must parse SQL");

    let schema_sym = parsed.symbols.iter().find(|s| s.symbol == "analytics");
    assert!(schema_sym.is_some(), "must extract analytics schema");
    assert_eq!(schema_sym.unwrap().kind, SymbolKind::Class);

    let type_sym = parsed.symbols.iter().find(|s| s.symbol == "order_status");
    assert!(type_sym.is_some(), "must extract order_status type");
    assert_eq!(type_sym.unwrap().kind, SymbolKind::Struct);
}
