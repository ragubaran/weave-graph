use weave_graph_store_sqlite::SqliteStorage;

use super::*;

fn storage_with_unresolved(refs: &[&str]) -> SqliteStorage {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let owned: Vec<String> = refs.iter().map(|s| s.to_string()).collect();
    storage
        .upsert_unresolved_refs("local", "a.rs", &owned)
        .unwrap();
    storage
}

#[test]
fn passes_when_the_file_has_no_unresolved_refs() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let result = weave_verify(
        &storage,
        VerifyArgs {
            file: "a.rs",
            range: None,
        },
    )
    .unwrap();
    assert_eq!(result.status, "pass");
    assert!(result.text.contains("a.rs"));
}

#[test]
fn fails_and_lists_every_unresolved_reference() {
    let storage = storage_with_unresolved(&["helper", "widget"]);
    let result = weave_verify(
        &storage,
        VerifyArgs {
            file: "a.rs",
            range: None,
        },
    )
    .unwrap();
    assert_eq!(result.status, "fail");
    assert!(result.text.contains("helper"));
    assert!(result.text.contains("widget"));
}

#[test]
fn range_is_carried_into_the_rendered_target() {
    let storage = storage_with_unresolved(&["helper"]);
    let result = weave_verify(
        &storage,
        VerifyArgs {
            file: "a.rs",
            range: Some((10, 20)),
        },
    )
    .unwrap();
    assert!(result.text.contains("a.rs:10-20"), "{}", result.text);
}

#[test]
fn is_scoped_to_the_named_file_only() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_unresolved_refs("local", "b.rs", &["other".to_string()])
        .unwrap();
    let result = weave_verify(
        &storage,
        VerifyArgs {
            file: "a.rs",
            range: None,
        },
    )
    .unwrap();
    assert_eq!(result.status, "pass", "{}", result.text);
}
