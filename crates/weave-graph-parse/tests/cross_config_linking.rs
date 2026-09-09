use std::path::Path;

use weave_graph_parse::{ProjectIndex, parse_file};

#[test]
fn typescript_process_env_links_to_dotenv_symbol() {
    let ts_source = r#"
function startServer() {
    const port = process.env.PORT;
    const dbUrl = process.env["DATABASE_URL"];
    return port;
}
"#;
    let env_source = r#"
PORT=3000
DATABASE_URL=postgres://localhost:5432/db
"#;

    let ts_file = parse_file(Path::new("server.ts"), ts_source)
        .unwrap()
        .expect("parse ts");
    let env_file = parse_file(Path::new(".env"), env_source)
        .unwrap()
        .expect("parse .env");

    let mut index = ProjectIndex::new();
    index.add_file(&ts_file);
    index.add_file(&env_file);

    let edges = index.resolve(&ts_file);
    assert!(
        !edges.is_empty(),
        "must produce resolved edges from TS to .env"
    );

    let linked_to_port = edges.iter().any(|e| {
        e.source_moniker == "server.ts#startServer"
            && e.target_moniker == ".env#PORT"
            && e.kind == "IMPORTS"
    });
    assert!(linked_to_port, "must link startServer to .env#PORT");

    let linked_to_db = edges.iter().any(|e| {
        e.source_moniker == "server.ts#startServer"
            && e.target_moniker == ".env#DATABASE_URL"
            && e.kind == "IMPORTS"
    });
    assert!(linked_to_db, "must link startServer to .env#DATABASE_URL");
}

#[test]
fn rust_env_macro_links_to_dotenv_symbol() {
    let rs_source = r#"
fn get_config() {
    let port = env!("PORT");
}
"#;
    let env_source = r#"
PORT=8080
"#;

    let rs_file = parse_file(Path::new("main.rs"), rs_source)
        .unwrap()
        .expect("parse rs");
    let env_file = parse_file(Path::new(".env"), env_source)
        .unwrap()
        .expect("parse .env");

    let mut index = ProjectIndex::new();
    index.add_file(&rs_file);
    index.add_file(&env_file);

    let edges = index.resolve(&rs_file);
    let linked_to_port = edges.iter().any(|e| {
        e.source_moniker == "main.rs#get_config"
            && e.target_moniker == ".env#PORT"
            && e.kind == "IMPORTS"
    });
    assert!(linked_to_port, "must link main.rs#get_config to .env#PORT");
}

#[test]
fn docker_compose_depends_on_links_services() {
    let yaml_source = r#"
services:
  web:
    image: nginx
    depends_on:
      - db
  db:
    image: postgres
"#;
    let compose_file = parse_file(Path::new("docker-compose.yml"), yaml_source)
        .unwrap()
        .expect("parse yaml");

    let mut index = ProjectIndex::new();
    index.add_file(&compose_file);

    let edges = index.resolve(&compose_file);
    let has_dep = edges.iter().any(|e| {
        e.source_moniker == "docker-compose.yml#depends_on"
            && e.target_moniker == "docker-compose.yml#db"
            && e.kind == "IMPORTS"
    });
    assert!(has_dep, "must link depends_on to docker-compose.yml#db");
}
