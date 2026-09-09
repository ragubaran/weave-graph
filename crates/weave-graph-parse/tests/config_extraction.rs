use std::path::Path;

use weave_graph_parse::{SymbolKind, parse_file};

#[test]
fn json_extraction_extracts_keys_and_monikers() {
    let source = r#"{
  "name": "my-app",
  "version": "1.0.0",
  "dependencies": {
    "react": "^18.2.0"
  }
}"#;
    let parsed = parse_file(Path::new("package.json"), source)
        .unwrap()
        .expect("must parse JSON");
    assert!(!parsed.symbols.is_empty(), "must extract symbols from JSON");

    let has_name = parsed
        .symbols
        .iter()
        .any(|s| s.symbol == "name" && s.kind == SymbolKind::Struct);
    let has_version = parsed.symbols.iter().any(|s| s.symbol == "version");
    let has_deps = parsed.symbols.iter().any(|s| s.symbol == "dependencies");

    assert!(has_name, "must extract 'name' key");
    assert!(has_version, "must extract 'version' key");
    assert!(has_deps, "must extract 'dependencies' key");
}

#[test]
fn yaml_extraction_extracts_keys_and_dependencies() {
    let source = r#"
version: '3.8'
services:
  web:
    image: nginx
    depends_on:
      - db
  db:
    image: postgres
"#;
    let parsed = parse_file(Path::new("docker-compose.yml"), source)
        .unwrap()
        .expect("must parse YAML");
    assert!(!parsed.symbols.is_empty(), "must extract symbols from YAML");

    let has_services = parsed.symbols.iter().any(|s| s.symbol == "services");
    let has_web = parsed.symbols.iter().any(|s| s.symbol == "web");
    let has_db = parsed.symbols.iter().any(|s| s.symbol == "db");

    assert!(has_services, "must extract 'services'");
    assert!(has_web, "must extract 'web'");
    assert!(has_db, "must extract 'db'");
}

#[test]
fn toml_extraction_extracts_tables_and_keys() {
    let source = r#"
[package]
name = "weave-graph"
version = "0.1.0"

[dependencies]
serde = "1.0"
"#;
    let parsed = parse_file(Path::new("Cargo.toml"), source)
        .unwrap()
        .expect("must parse TOML");
    assert!(!parsed.symbols.is_empty(), "must extract symbols from TOML");

    let has_pkg = parsed.symbols.iter().any(|s| s.symbol == "package");
    let has_name = parsed.symbols.iter().any(|s| s.symbol == "name");
    let has_deps = parsed.symbols.iter().any(|s| s.symbol == "dependencies");

    assert!(has_pkg, "must extract 'package' table");
    assert!(has_name, "must extract 'name' key");
    assert!(has_deps, "must extract 'dependencies' table");
}

#[test]
fn properties_and_env_extraction_extracts_keys() {
    let props_source = r#"
server.port=8080
spring.datasource.url=jdbc:postgresql://localhost:5432/mydb
"#;
    let parsed_props = parse_file(Path::new("application.properties"), props_source)
        .unwrap()
        .expect("must parse properties");
    assert!(!parsed_props.symbols.is_empty());
    assert!(
        parsed_props
            .symbols
            .iter()
            .any(|s| s.symbol == "server.port")
    );
    assert!(
        parsed_props
            .symbols
            .iter()
            .any(|s| s.symbol == "spring.datasource.url")
    );

    let env_source = r#"
PORT=3000
DATABASE_URL=postgres://user:pass@localhost/db
SECRET_KEY=supersecret
"#;
    let parsed_env = parse_file(Path::new(".env"), env_source)
        .unwrap()
        .expect("must parse .env");
    assert!(!parsed_env.symbols.is_empty());
    assert!(parsed_env.symbols.iter().any(|s| s.symbol == "PORT"));
    assert!(
        parsed_env
            .symbols
            .iter()
            .any(|s| s.symbol == "DATABASE_URL")
    );
}
