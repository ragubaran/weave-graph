use std::fs;

use super::*;

#[test]
fn read_storage_home_returns_none_when_file_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_storage_home(&dir.path().join("config.toml")).is_none());
}

#[test]
fn read_storage_home_returns_none_when_section_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "mode = \"single\"\n").unwrap();
    assert!(read_storage_home(&path).is_none());
}

#[test]
fn read_storage_home_parses_the_configured_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        "mode = \"single\"\n\n[storage]\nhome = \"/local/weave-data\"\n",
    )
    .unwrap();
    assert_eq!(
        read_storage_home(&path),
        Some(PathBuf::from("/local/weave-data"))
    );
}

#[test]
fn read_storage_home_returns_none_on_malformed_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "not valid = = toml").unwrap();
    assert!(read_storage_home(&path).is_none());
}

#[test]
fn set_key_creates_the_file_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    set_key(&path, "mode", "multiple").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("mode = \"multiple\""));
}

#[test]
fn set_key_preserves_other_top_level_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "mode = \"single\"\nother = \"kept\"\n").unwrap();
    set_key(&path, "mode", "multiple").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("mode = \"multiple\""));
    assert!(content.contains("other = \"kept\""));
}

#[test]
fn set_key_handles_a_dotted_nested_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    set_key(&path, "storage.home", "/local/data").unwrap();
    assert_eq!(read_storage_home(&path), Some(PathBuf::from("/local/data")));
}

#[test]
fn set_key_overwrites_a_non_table_value_that_is_in_the_way() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "storage = \"oops\"\n").unwrap();
    set_key(&path, "storage.home", "/local/data").unwrap();
    assert_eq!(read_storage_home(&path), Some(PathBuf::from("/local/data")));
}

#[test]
fn set_key_is_idempotent_when_run_twice() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    set_key(&path, "mode", "multiple").unwrap();
    set_key(&path, "mode", "multiple").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(content.matches("mode").count(), 1);
}

#[test]
fn parse_value_recognizes_real_toml_types() {
    assert_eq!(parse_value("true"), toml::Value::Boolean(true));
    assert_eq!(parse_value("42"), toml::Value::Integer(42));
    assert_eq!(
        parse_value("single"),
        toml::Value::String("single".to_string())
    );
}

#[test]
fn get_key_reads_top_level_and_nested_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    set_key(&path, "mode", "single").unwrap();
    set_key(&path, "storage.home", "/custom/path").unwrap();

    assert_eq!(get_key(&path, "mode"), Some("single".to_string()));
    assert_eq!(
        get_key(&path, "storage.home"),
        Some("/custom/path".to_string())
    );
    assert_eq!(get_key(&path, "storage.nonexistent"), None);
    assert_eq!(get_key(&path, "nonexistent"), None);
}

#[test]
fn read_linked_repos_parses_a_toml_array_of_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        "[federation]\nlinked_repos = [\"../a\", \"../b\"]\nstaleness_policy = \"warn\"\n",
    )
    .unwrap();

    assert_eq!(
        read_linked_repos(&path),
        vec![PathBuf::from("../a"), PathBuf::from("../b")]
    );
}

#[test]
fn read_linked_repos_is_empty_for_missing_file_or_section_or_scalar() {
    let dir = tempfile::tempdir().unwrap();
    // Missing file.
    assert!(read_linked_repos(&dir.path().join("nope.toml")).is_empty());

    // File without the federation section.
    let path = dir.path().join("config.toml");
    fs::write(&path, "mode = \"single\"\n").unwrap();
    assert!(read_linked_repos(&path).is_empty());

    // Section present but linked_repos is a scalar, not an array.
    fs::write(&path, "[federation]\nlinked_repos = \"../a\"\n").unwrap();
    assert!(read_linked_repos(&path).is_empty());
}

#[test]
fn read_linked_repos_tolerates_an_empty_array() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[federation]\nlinked_repos = []\n").unwrap();
    assert!(read_linked_repos(&path).is_empty());
}

#[test]
fn read_rbac_users_returns_empty_map_when_file_or_section_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_rbac_users(&dir.path().join("nope.toml")).is_empty());

    let path = dir.path().join("config.toml");
    fs::write(&path, "mode = \"single\"\n").unwrap();
    assert!(read_rbac_users(&path).is_empty());
}

#[test]
fn read_rbac_users_parses_the_configured_map() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[rbac.users.alice]\nroles = [\"internal\"]\ntoken = \"t1\"\n\n[rbac.users.bob]\nroles = []\ntoken = \"t2\"\n").unwrap();
    let users = read_rbac_users(&path);
    assert_eq!(
        users.get("alice").map(|u| u.roles.clone()),
        Some(vec!["internal".to_string()])
    );
    assert_eq!(
        users.get("alice").and_then(|u| u.token.clone()),
        Some("t1".to_string())
    );
    assert_eq!(users.get("bob").map(|u| u.roles.clone()), Some(vec![]));
    assert_eq!(
        users.get("bob").and_then(|u| u.token.clone()),
        Some("t2".to_string())
    );
    assert!(!users.contains_key("carol"));
}

#[test]
fn read_rbac_users_parses_path_scope_from_the_table_form() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        "[rbac.users.hub_oncall]\nroles = [\"internal\"]\npath_scope = [\"crates/weave-graph-hub\"]\n\n[rbac.users.alice]\nroles = [\"internal\"]\n",
    )
    .unwrap();
    let users = read_rbac_users(&path);
    assert_eq!(
        users.get("hub_oncall").map(|u| u.path_scope.clone()),
        Some(vec!["crates/weave-graph-hub".to_string()])
    );
    // Absent `path_scope` is empty, not a parse failure.
    assert_eq!(
        users.get("alice").map(|u| u.path_scope.clone()),
        Some(vec![])
    );
    // The bare-array shorthand (`alice = ["internal"]`) has no table to
    // hold a `path_scope`, so it's simply unscoped — not an error.
}

#[test]
fn read_rbac_users_bare_array_shorthand_has_no_path_scope() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[rbac.users]\nalice = [\"internal\"]\n").unwrap();
    let users = read_rbac_users(&path);
    assert_eq!(
        users.get("alice").map(|u| u.path_scope.clone()),
        Some(vec![])
    );
}
